{{/*
Expand the name of the chart.
*/}}
{{- define "vortex.name" -}}
{{- default .Chart.Name .Values.nameOverride | trunc 63 | trimSuffix "-" }}
{{- end }}

{{/*
Create a default fully qualified app name.
*/}}
{{- define "vortex.fullname" -}}
{{- if .Values.fullnameOverride }}
{{- .Values.fullnameOverride | trunc 63 | trimSuffix "-" }}
{{- else }}
{{- $name := default .Chart.Name .Values.nameOverride }}
{{- if contains $name .Release.Name }}
{{- .Release.Name | trunc 63 | trimSuffix "-" }}
{{- else }}
{{- printf "%s-%s" .Release.Name $name | trunc 63 | trimSuffix "-" }}
{{- end }}
{{- end }}
{{- end }}

{{/*
Common labels
*/}}
{{- define "vortex.labels" -}}
helm.sh/chart: {{ include "vortex.name" . }}-{{ .Chart.Version | replace "+" "_" }}
{{ include "vortex.selectorLabels" . }}
app.kubernetes.io/version: {{ .Chart.AppVersion | quote }}
app.kubernetes.io/managed-by: {{ .Release.Service }}
{{- end }}

{{/*
Selector labels
*/}}
{{- define "vortex.selectorLabels" -}}
app.kubernetes.io/name: {{ include "vortex.name" . }}
app.kubernetes.io/instance: {{ .Release.Name }}
{{- end }}

{{/*
Service account name
*/}}
{{- define "vortex.serviceAccountName" -}}
{{- if .Values.serviceAccount.create }}
{{- default (include "vortex.fullname" .) .Values.serviceAccount.name }}
{{- else }}
{{- default "default" .Values.serviceAccount.name }}
{{- end }}
{{- end }}

{{/*
Image tag (default to appVersion)
*/}}
{{- define "vortex.imageTag" -}}
{{- default .Chart.AppVersion .Values.image.tag }}
{{- end }}

{{/*
Database URL
*/}}
{{- define "vortex.databaseUrl" -}}
{{- if .Values.postgresql.enabled -}}
postgres://{{ .Values.postgresql.auth.username }}:$(DATABASE_PASSWORD)@{{ include "vortex.fullname" . }}-postgresql:5432/{{ .Values.postgresql.auth.database }}
{{- else -}}
postgres://{{ .Values.externalDatabase.username }}:$(DATABASE_PASSWORD)@{{ .Values.externalDatabase.host }}:{{ .Values.externalDatabase.port }}/{{ .Values.externalDatabase.database }}
{{- end -}}
{{- end }}

{{/*
DAG volume mounts
*/}}
{{- define "vortex.dagVolumeMounts" -}}
- name: dags
  mountPath: /app/dags
  readOnly: true
{{- end }}

{{/*
DAG volumes
*/}}
{{- define "vortex.dagVolumes" -}}
- name: dags
  persistentVolumeClaim:
    claimName: {{ include "vortex.fullname" . }}-dags
{{- end }}
