{{/*
Expand the name of the chart.
*/}}
{{- define "ryuo.name" -}}
{{- default .Chart.Name .Values.nameOverride | trunc 63 | trimSuffix "-" }}
{{- end }}

{{/*
Create a default fully qualified app name.
*/}}
{{- define "ryuo.fullname" -}}
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
{{- define "ryuo.labels" -}}
helm.sh/chart: {{ include "ryuo.name" . }}-{{ .Chart.Version | replace "+" "_" }}
{{ include "ryuo.selectorLabels" . }}
app.kubernetes.io/version: {{ .Chart.AppVersion | quote }}
app.kubernetes.io/managed-by: {{ .Release.Service }}
{{- end }}

{{/*
Selector labels
*/}}
{{- define "ryuo.selectorLabels" -}}
app.kubernetes.io/name: {{ include "ryuo.name" . }}
app.kubernetes.io/instance: {{ .Release.Name }}
{{- end }}

{{/*
Service account name
*/}}
{{- define "ryuo.serviceAccountName" -}}
{{- if .Values.serviceAccount.create }}
{{- default (include "ryuo.fullname" .) .Values.serviceAccount.name }}
{{- else }}
{{- default "default" .Values.serviceAccount.name }}
{{- end }}
{{- end }}

{{/*
Image tag (default to appVersion)
*/}}
{{- define "ryuo.imageTag" -}}
{{- default .Chart.AppVersion .Values.image.tag }}
{{- end }}

{{/*
Database URL
*/}}
{{- define "ryuo.databaseUrl" -}}
{{- if .Values.postgresql.enabled -}}
postgres://{{ .Values.postgresql.auth.username }}:$(DATABASE_PASSWORD)@{{ include "ryuo.fullname" . }}-postgresql:5432/{{ .Values.postgresql.auth.database }}
{{- else -}}
postgres://{{ .Values.externalDatabase.username }}:$(DATABASE_PASSWORD)@{{ .Values.externalDatabase.host }}:{{ .Values.externalDatabase.port }}/{{ .Values.externalDatabase.database }}
{{- end -}}
{{- end }}

{{/*
DAG volume mounts
*/}}
{{- define "ryuo.dagVolumeMounts" -}}
- name: dags
  mountPath: /app/dags
  readOnly: true
{{- end }}

{{/*
DAG volumes
*/}}
{{- define "ryuo.dagVolumes" -}}
- name: dags
  persistentVolumeClaim:
    claimName: {{ include "ryuo.fullname" . }}-dags
{{- end }}
