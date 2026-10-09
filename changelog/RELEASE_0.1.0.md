# Release 0.1.0

**Released:** 2026-10-09
**Previous version:** none (first release)

The first release of Local Mates: a dead-simple virtual LAN for playing LAN games with friends over the internet. Open your room, share its short code, and your friends join as if they were on the same network. Windows-first, with a desktop app and a signed installer that keeps itself up to date.

## New Features

- Added a virtual LAN that connects friends peer to peer, with encrypted direct connections where possible and relays when not. LAN game discovery broadcasts reach everyone in the room, so games find each other as if on the same network (#1)
- Added persistent rooms with short codes like `K7M-Q2X`. Your room keeps its code until you reset it, and rooms you've joined are saved so you can hop back in by name (#1)
- Added host approval: the host is asked before a new device can join, and approved friends get straight in next time (#1)
- Added the Local Mates desktop app: open or close your room, join a friend's code, approve new devices, and see recent activity. Setup takes a single admin prompt, and closing the window keeps it running in the tray, where it pops back up when someone wants to join (#2)
- Added the `local-mates` command line, which does everything the app does (#1)
- Added automatic updates: new versions download in the background and install the next time you start Local Mates, so a running session is never interrupted (#1)

## Improvements

- Windows marks the Local Mates adapter as a Private network with the lowest route metric, so the firewall doesn't block games and discovery broadcasts go out over Local Mates rather than your real network (#1)
- The Windows installer and app are code-signed by Starkey Digital Ltd (#4)
