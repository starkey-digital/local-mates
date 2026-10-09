{{- define "relay.name" -}}
{{- printf "%s-relay" .Release.Name -}}
{{- end -}}

{{- define "relay.tlsSecret" -}}
{{- .Values.relay.tls.secretName | default (printf "%s-tls" (include "relay.name" .)) -}}
{{- end -}}

{{- define "rooms.name" -}}
{{- printf "%s-rooms" .Release.Name -}}
{{- end -}}

{{- define "image" -}}
{{- printf "%s:%s" .image.repository (.image.tag | default .appVersion) -}}
{{- end -}}

{{/* Pod/container hardening that satisfies the "restricted" Pod Security Standard. */}}
{{- define "podSecurity" -}}
runAsNonRoot: true
runAsUser: 65532
runAsGroup: 65532
seccompProfile:
  type: RuntimeDefault
{{- end -}}

{{- define "containerSecurity" -}}
allowPrivilegeEscalation: false
readOnlyRootFilesystem: true
capabilities:
  drop: [ALL]
{{- end -}}
