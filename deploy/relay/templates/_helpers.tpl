{{- define "relay.tlsSecret" -}}
{{- .Values.tls.secretName | default (printf "%s-tls" .Release.Name) -}}
{{- end -}}
