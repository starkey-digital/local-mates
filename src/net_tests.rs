//! End-to-end tests of hosting and joining: real iroh endpoints talking over localhost, with an
//! in-memory adapter standing in for Wintun/utun so no admin rights or network setup is needed.

use std::{
    io,
    net::{Ipv4Addr, SocketAddr},
    sync::{Arc, Mutex},
    time::Duration,
};

use anyhow::Result;
use iroh::{Endpoint, EndpointAddr, RelayMode, TransportAddr, endpoint::presets};
use tokio::{
    sync::{broadcast, mpsc, oneshot},
    task::JoinHandle,
    time::timeout,
};

use crate::{
    host,
    ipc::Event,
    join, link,
    packet::{HOST_IP, peer_ip},
    session::Shared,
    store::{Device, Store},
    tun::Adapter,
};

const WAIT: Duration = Duration::from_secs(10);
/// How long to wait before concluding a packet is (correctly) not coming.
const QUIET: Duration = Duration::from_millis(500);

struct FakeAdapter {
    from_os: tokio::sync::Mutex<mpsc::UnboundedReceiver<Vec<u8>>>,
    to_os: mpsc::UnboundedSender<Vec<u8>>,
}

impl Adapter for FakeAdapter {
    async fn recv(&self, buf: &mut [u8]) -> io::Result<usize> {
        let pkt = self
            .from_os
            .lock()
            .await
            .recv()
            .await
            .ok_or(io::ErrorKind::BrokenPipe)?;
        buf[..pkt.len()].copy_from_slice(&pkt);
        Ok(pkt.len())
    }

    async fn send(&self, pkt: &[u8]) -> io::Result<usize> {
        let _ = self.to_os.send(pkt.to_vec());
        Ok(pkt.len())
    }
}

/// The test's side of a fake adapter: what the "OS" sends into the tunnel, and what comes out.
struct Wire {
    ip: Ipv4Addr,
    send: mpsc::UnboundedSender<Vec<u8>>,
    recv: mpsc::UnboundedReceiver<Vec<u8>>,
}

impl Wire {
    fn send(&self, dst: Ipv4Addr, payload: &[u8]) {
        self.send_from(self.ip, dst, payload);
    }

    fn send_from(&self, src: Ipv4Addr, dst: Ipv4Addr, payload: &[u8]) {
        self.send.send(ipv4(src, dst, payload)).unwrap();
    }

    async fn expect(&mut self, payload: &[u8]) {
        let pkt = timeout(WAIT, self.recv.recv())
            .await
            .expect("packet didn't arrive")
            .unwrap();
        assert_eq!(&pkt[20..], payload, "unexpected packet at {}", self.ip);
    }

    async fn expect_nothing(&mut self) {
        if let Ok(Some(pkt)) = timeout(QUIET, self.recv.recv()).await {
            panic!("{} got an unexpected packet: {:?}", self.ip, &pkt[20..]);
        }
    }
}

fn fake_adapter() -> (
    impl FnOnce(Ipv4Addr) -> Result<FakeAdapter>,
    oneshot::Receiver<Wire>,
) {
    let (wire_tx, wire_rx) = oneshot::channel();
    let open = move |ip| {
        let (send, from_os) = mpsc::unbounded_channel();
        let (to_os, recv) = mpsc::unbounded_channel();
        let _ = wire_tx.send(Wire { ip, send, recv });
        Ok(FakeAdapter {
            from_os: from_os.into(),
            to_os,
        })
    };
    (open, wire_rx)
}

fn ipv4(src: Ipv4Addr, dst: Ipv4Addr, payload: &[u8]) -> Vec<u8> {
    let mut pkt = vec![0; 20];
    pkt[0] = 0x45;
    pkt[12..16].copy_from_slice(&src.octets());
    pkt[16..20].copy_from_slice(&dst.octets());
    pkt.extend_from_slice(payload);
    pkt
}

struct Node {
    endpoint: Endpoint,
    shared: Shared,
    events: broadcast::Receiver<Event>,
    _dir: tempfile::TempDir,
}

impl Node {
    async fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::at(dir.path().join("state.json"));
        let shared = Shared {
            events: broadcast::channel(64).0,
            store: Arc::new(Mutex::new(store)),
            rooms_api: None,
            approvals: Arc::default(),
        };
        let key = shared.store.lock().unwrap().key().unwrap();
        let endpoint = Endpoint::builder(presets::Minimal)
            .secret_key(key)
            .relay_mode(RelayMode::Disabled)
            .alpns(vec![link::ALPN.to_vec()])
            .bind()
            .await
            .unwrap();
        Self {
            events: shared.events.subscribe(),
            endpoint,
            shared,
            _dir: dir,
        }
    }

    /// Dialable over loopback, since tests run without relays or discovery.
    fn addr(&self) -> EndpointAddr {
        let port = self
            .endpoint
            .bound_sockets()
            .iter()
            .find(|a| a.is_ipv4())
            .expect("bound to IPv4")
            .port();
        let local = SocketAddr::from((Ipv4Addr::LOCALHOST, port));
        EndpointAddr::from_parts(self.endpoint.id(), [TransportAddr::Ip(local)])
    }

    fn device(&self) -> Device {
        Device {
            name: link::device_name(),
            endpoint_id: self.endpoint.id().to_string(),
        }
    }

    fn befriend(&self, other: &Node) {
        self.shared
            .store
            .lock()
            .unwrap()
            .add_friend(other.device())
            .unwrap();
    }

    async fn host(&self) -> (JoinHandle<Result<()>>, Wire) {
        let (open, wire) = fake_adapter();
        let (endpoint, shared) = (self.endpoint.clone(), self.shared.clone());
        let task = tokio::spawn(async move { host::run(endpoint, &shared, open).await });
        (task, timeout(WAIT, wire).await.unwrap().unwrap())
    }

    fn join(&self, host: &Node) -> (JoinHandle<Result<()>>, oneshot::Receiver<Wire>) {
        let (open, wire) = fake_adapter();
        let (endpoint, shared, addr) = (self.endpoint.clone(), self.shared.clone(), host.addr());
        let task = tokio::spawn(async move { join::run(endpoint, addr, &shared, open).await });
        (task, wire)
    }

    /// Joins as an existing friend and waits until the tunnel is up.
    async fn join_as_friend(&self, host: &Node) -> (JoinHandle<Result<()>>, Wire) {
        host.befriend(self);
        let (task, wire) = self.join(host);
        (
            task,
            timeout(WAIT, wire).await.expect("join timed out").unwrap(),
        )
    }

    async fn next_event(&mut self, wanted: impl Fn(&Event) -> bool) -> Event {
        timeout(WAIT, async {
            loop {
                let event = self.events.recv().await.unwrap();
                if wanted(&event) {
                    return event;
                }
            }
        })
        .await
        .expect("event didn't arrive")
    }

    fn answer(&self, id: &str, allow: bool) {
        let answer = self
            .shared
            .approvals
            .lock()
            .unwrap()
            .remove(&id.parse::<iroh::EndpointId>().unwrap());
        answer.expect("no pending request").send(allow).unwrap();
    }
}

#[tokio::test]
async fn friend_joins_and_packets_flow_both_ways() {
    let (host, guest) = (Node::new().await, Node::new().await);
    let (_h, mut host_wire) = host.host().await;
    let (_g, mut guest_wire) = guest.join_as_friend(&host).await;

    assert_eq!(host_wire.ip, HOST_IP);
    assert_eq!(guest_wire.ip, peer_ip(2));

    guest_wire.send(HOST_IP, b"hello host");
    host_wire.expect(b"hello host").await;
    host_wire.send(guest_wire.ip, b"hello guest");
    guest_wire.expect(b"hello guest").await;

    let rooms = guest.shared.store.lock().unwrap().rooms().to_vec();
    assert_eq!(rooms.len(), 1);
    assert_eq!(rooms[0].endpoint_id, host.endpoint.id().to_string());
    assert_eq!(rooms[0].name, format!("{}'s room", link::device_name()));
}

#[tokio::test]
async fn stranger_waits_for_approval_and_is_remembered() {
    let (mut host, mut guest) = (Node::new().await, Node::new().await);
    let (_h, _host_wire) = host.host().await;
    let (_g, wire) = guest.join(&host);

    let Event::JoinRequest { id, name } = host
        .next_event(|e| matches!(e, Event::JoinRequest { .. }))
        .await
    else {
        unreachable!()
    };
    assert_eq!(id, guest.endpoint.id().to_string());
    assert_eq!(name, link::device_name());
    guest.next_event(|e| matches!(e, Event::Waiting)).await;

    host.answer(&id, true);
    let wire = timeout(WAIT, wire).await.expect("join timed out").unwrap();
    assert_eq!(wire.ip, peer_ip(2));
    assert!(host.shared.store.lock().unwrap().is_friend(&id));
}

#[tokio::test]
async fn refused_stranger_is_told_and_nothing_is_saved() {
    let (mut host, guest) = (Node::new().await, Node::new().await);
    let (_h, _host_wire) = host.host().await;
    let (task, _wire) = guest.join(&host);

    let Event::JoinRequest { id, .. } = host
        .next_event(|e| matches!(e, Event::JoinRequest { .. }))
        .await
    else {
        unreachable!()
    };
    host.answer(&id, false);

    let err = timeout(WAIT, task).await.unwrap().unwrap().unwrap_err();
    assert!(err.to_string().contains("didn't let you in"), "{err:#}");
    assert!(!host.shared.store.lock().unwrap().is_friend(&id));
    assert!(guest.shared.store.lock().unwrap().rooms().is_empty());
}

#[tokio::test]
async fn host_switches_broadcast_and_unicast_between_guests() {
    let (host, a, b) = (Node::new().await, Node::new().await, Node::new().await);
    let (_h, mut host_wire) = host.host().await;
    let (_a, mut a_wire) = a.join_as_friend(&host).await;
    let (_b, mut b_wire) = b.join_as_friend(&host).await;
    assert_eq!((a_wire.ip, b_wire.ip), (peer_ip(2), peer_ip(3)));

    // LAN game discovery: everyone but the sender hears it.
    a_wire.send(Ipv4Addr::BROADCAST, b"anyone hosting?");
    host_wire.expect(b"anyone hosting?").await;
    b_wire.expect(b"anyone hosting?").await;
    a_wire.expect_nothing().await;

    // Guest to guest goes through the host without reaching its adapter.
    a_wire.send(b_wire.ip, b"direct to b");
    b_wire.expect(b"direct to b").await;
    host_wire.expect_nothing().await;

    // The host's own broadcasts reach every guest.
    host_wire.send(Ipv4Addr::new(10, 77, 0, 255), b"subnet broadcast");
    a_wire.expect(b"subnet broadcast").await;
    b_wire.expect(b"subnet broadcast").await;
}

#[tokio::test]
async fn spoofed_source_is_dropped() {
    let (host, a, b) = (Node::new().await, Node::new().await, Node::new().await);
    let (_h, mut host_wire) = host.host().await;
    let (_a, a_wire) = a.join_as_friend(&host).await;
    let (_b, mut b_wire) = b.join_as_friend(&host).await;

    // a pretending to be b.
    a_wire.send_from(b_wire.ip, HOST_IP, b"forged");
    host_wire.expect_nothing().await;
    a_wire.send_from(b_wire.ip, Ipv4Addr::BROADCAST, b"forged");
    b_wire.expect_nothing().await;
}

#[tokio::test]
async fn leaving_frees_the_address() {
    let (host, a, b) = (Node::new().await, Node::new().await, Node::new().await);
    let (_h, _host_wire) = host.host().await;
    let (a_task, a_wire) = a.join_as_friend(&host).await;
    assert_eq!(a_wire.ip, peer_ip(2));

    a_task.abort();
    a.endpoint.close().await;
    // The host notices the closed connection and releases .2 for the next guest.
    let (_b, b_wire) = b.join_as_friend(&host).await;
    assert_eq!(b_wire.ip, peer_ip(2));
}
