pub(in crate::generator) fn app_service_tpl() -> String {
    r#"{{- range $id, $service := .Values.services }}
apiVersion: v1
kind: Service
metadata:
  name: {{ include "deployment.resourceName" (dict "root" $ "name" $id) }}
  labels:
    {{- include "deployment.labels" $ | nindent 4 }}
    resource-id: {{ $id | quote }}
spec:
  type: {{ if eq $service.type "loadBalancer" }}LoadBalancer{{ else }}ClusterIP{{ end }}
  selector:
    app: {{ include "deployment.resourceName" (dict "root" $ "name" $id) }}
    managed-by: runtime
    component: {{ $service.component | quote }}
  ports:
    - name: http
      port: {{ default 80 $service.port }}
      targetPort: {{ default 8080 $service.targetPort }}
---
{{- end }}
"#
    .to_string()
}

pub(in crate::generator) fn cluster_bootstrap_tpl() -> String {
    r#"{{- $bootstrap := default dict .Values.clusterBootstrap -}}
{{- $storage := dig "storageClass" "default" dict $bootstrap -}}
{{- if dig "enabled" false $storage }}
{{- $storageName := required "clusterBootstrap.storageClass.default.name is required when enabled" $storage.name -}}
{{- $provisioner := required "clusterBootstrap.storageClass.default.provisioner is required when enabled" $storage.provisioner -}}
apiVersion: storage.k8s.io/v1
kind: StorageClass
metadata:
  name: {{ $storageName | quote }}
  annotations:
    storageclass.kubernetes.io/is-default-class: "true"
  labels:
    {{- include "deployment.labels" . | nindent 4 }}
provisioner: {{ $provisioner | quote }}
{{ with $storage.parameters }}
parameters:
  {{ range $key, $value := . }}
  {{ $key }}: {{ $value | quote }}
  {{ end }}
{{ end }}
reclaimPolicy: Delete
volumeBindingMode: WaitForFirstConsumer
allowVolumeExpansion: true
{{- if eq $provisioner "ebs.csi.eks.amazonaws.com" }}
allowedTopologies:
  - matchLabelExpressions:
      - key: eks.amazonaws.com/compute-type
        values:
          - auto
{{ end }}
{{ end }}
{{- $eksAlb := dig "ingress" "eksAutoMode" dict $bootstrap -}}
{{- if dig "enabled" false $eksAlb }}
{{- $ingressClassName := required "clusterBootstrap.ingress.eksAutoMode.name is required when enabled" $eksAlb.name -}}
{{- $controller := required "clusterBootstrap.ingress.eksAutoMode.controller is required when enabled" $eksAlb.controller -}}
---
apiVersion: eks.amazonaws.com/v1
kind: IngressClassParams
metadata:
  name: {{ $ingressClassName | quote }}
  labels:
    {{- include "deployment.labels" . | nindent 4 }}
spec:
  scheme: {{ default "internet-facing" $eksAlb.scheme | quote }}
  {{ with $eksAlb.subnetIds }}
  subnets:
    ids:
      {{ range . }}
      - {{ . | quote }}
      {{ end }}
  {{ end }}
---
apiVersion: networking.k8s.io/v1
kind: IngressClass
metadata:
  name: {{ $ingressClassName | quote }}
  labels:
    {{- include "deployment.labels" . | nindent 4 }}
spec:
  controller: {{ $controller | quote }}
  parameters:
    apiGroup: eks.amazonaws.com
    kind: IngressClassParams
    name: {{ $ingressClassName | quote }}
{{ end }}
{{- $azureAgc := dig "ingress" "azureApplicationGatewayForContainers" dict $bootstrap -}}
{{- if dig "enabled" false $azureAgc }}
{{- $azureAlb := required "clusterBootstrap.ingress.azureApplicationGatewayForContainers.applicationLoadBalancer is required when enabled" $azureAgc.applicationLoadBalancer -}}
{{- $azureAlbName := required "clusterBootstrap.ingress.azureApplicationGatewayForContainers.applicationLoadBalancer.name is required when enabled" $azureAlb.name -}}
{{- $azureAlbNamespace := required "clusterBootstrap.ingress.azureApplicationGatewayForContainers.applicationLoadBalancer.namespace is required when enabled" $azureAlb.namespace -}}
{{- $azureAssociationSubnetId := required "clusterBootstrap.ingress.azureApplicationGatewayForContainers.applicationLoadBalancer.associationSubnetId is required when enabled" $azureAlb.associationSubnetId -}}
---
apiVersion: alb.networking.azure.io/v1
kind: ApplicationLoadBalancer
metadata:
  name: {{ $azureAlbName | quote }}
  namespace: {{ $azureAlbNamespace | quote }}
  labels:
    {{- include "deployment.labels" . | nindent 4 }}
spec:
  associations:
    - {{ $azureAssociationSubnetId | quote }}
{{ end }}
{{- $eksArm64NodePool := dig "compute" "eksAutoMode" "arm64NodePool" dict $bootstrap -}}
{{- if dig "enabled" false $eksArm64NodePool }}
{{- $nodePoolName := required "clusterBootstrap.compute.eksAutoMode.arm64NodePool.name is required when enabled" $eksArm64NodePool.name -}}
{{- $nodeClassName := required "clusterBootstrap.compute.eksAutoMode.arm64NodePool.nodeClassName is required when enabled" $eksArm64NodePool.nodeClassName -}}
---
apiVersion: karpenter.sh/v1
kind: NodePool
metadata:
  name: {{ $nodePoolName | quote }}
  labels:
    {{- include "deployment.labels" . | nindent 4 }}
spec:
  template:
    spec:
      nodeClassRef:
        group: eks.amazonaws.com
        kind: NodeClass
        name: {{ $nodeClassName | quote }}
      requirements:
        - key: karpenter.sh/capacity-type
          operator: In
          values:
            - {{ default "on-demand" $eksArm64NodePool.capacityType | quote }}
        - key: kubernetes.io/arch
          operator: In
          values:
            - "arm64"
        - key: eks.amazonaws.com/instance-category
          operator: In
          values:
            {{ range (default (list "c" "m" "r") $eksArm64NodePool.instanceCategories) }}
            - {{ . | quote }}
            {{ end }}
        - key: eks.amazonaws.com/instance-generation
          operator: Gt
          values:
            - {{ default "5" $eksArm64NodePool.minInstanceGeneration | quote }}
  {{ with $eksArm64NodePool.limits }}
  limits:
    {{ with .cpu }}
    cpu: {{ . | quote }}
    {{ end }}
    {{ with .memory }}
    memory: {{ . | quote }}
    {{ end }}
  {{ end }}
{{ end }}
{{- $metrics := dig "metricsServer" dict $bootstrap -}}
{{- if dig "enabled" false $metrics }}
---
apiVersion: v1
kind: ServiceAccount
metadata:
  name: metrics-server
  namespace: kube-system
  labels:
    k8s-app: metrics-server
    {{- include "deployment.labels" . | nindent 4 }}
---
apiVersion: rbac.authorization.k8s.io/v1
kind: ClusterRole
metadata:
  name: system:aggregated-metrics-reader
  labels:
    k8s-app: metrics-server
    rbac.authorization.k8s.io/aggregate-to-admin: "true"
    rbac.authorization.k8s.io/aggregate-to-edit: "true"
    rbac.authorization.k8s.io/aggregate-to-view: "true"
    {{- include "deployment.labels" . | nindent 4 }}
rules:
  - apiGroups: ["metrics.k8s.io"]
    resources: ["pods", "nodes"]
    verbs: ["get", "list", "watch"]
---
apiVersion: rbac.authorization.k8s.io/v1
kind: ClusterRole
metadata:
  name: system:metrics-server
  labels:
    k8s-app: metrics-server
    {{- include "deployment.labels" . | nindent 4 }}
rules:
  - apiGroups: [""]
    resources: ["nodes/metrics"]
    verbs: ["get"]
  - apiGroups: [""]
    resources: ["pods", "nodes"]
    verbs: ["get", "list", "watch"]
---
apiVersion: rbac.authorization.k8s.io/v1
kind: RoleBinding
metadata:
  name: metrics-server-auth-reader
  namespace: kube-system
  labels:
    k8s-app: metrics-server
    {{- include "deployment.labels" . | nindent 4 }}
roleRef:
  apiGroup: rbac.authorization.k8s.io
  kind: Role
  name: extension-apiserver-authentication-reader
subjects:
  - kind: ServiceAccount
    name: metrics-server
    namespace: kube-system
---
apiVersion: rbac.authorization.k8s.io/v1
kind: ClusterRoleBinding
metadata:
  name: metrics-server:system:auth-delegator
  labels:
    k8s-app: metrics-server
    {{- include "deployment.labels" . | nindent 4 }}
roleRef:
  apiGroup: rbac.authorization.k8s.io
  kind: ClusterRole
  name: system:auth-delegator
subjects:
  - kind: ServiceAccount
    name: metrics-server
    namespace: kube-system
---
apiVersion: rbac.authorization.k8s.io/v1
kind: ClusterRoleBinding
metadata:
  name: system:metrics-server
  labels:
    k8s-app: metrics-server
    {{- include "deployment.labels" . | nindent 4 }}
roleRef:
  apiGroup: rbac.authorization.k8s.io
  kind: ClusterRole
  name: system:metrics-server
subjects:
  - kind: ServiceAccount
    name: metrics-server
    namespace: kube-system
---
apiVersion: v1
kind: Service
metadata:
  name: metrics-server
  namespace: kube-system
  labels:
    k8s-app: metrics-server
    {{- include "deployment.labels" . | nindent 4 }}
spec:
  selector:
    k8s-app: metrics-server
  ports:
    - name: https
      port: 443
      protocol: TCP
      targetPort: https
---
apiVersion: apps/v1
kind: Deployment
metadata:
  name: metrics-server
  namespace: kube-system
  labels:
    k8s-app: metrics-server
    {{- include "deployment.labels" . | nindent 4 }}
spec:
  selector:
    matchLabels:
      k8s-app: metrics-server
  template:
    metadata:
      labels:
        k8s-app: metrics-server
    spec:
      serviceAccountName: metrics-server
      containers:
        - name: metrics-server
          image: {{ default "registry.k8s.io/metrics-server/metrics-server:v0.8.1" $metrics.image | quote }}
          imagePullPolicy: IfNotPresent
          args:
            - --cert-dir=/tmp
            - --secure-port=10250
            - --kubelet-preferred-address-types=InternalIP,ExternalIP,Hostname
            - --kubelet-use-node-status-port
            - --metric-resolution=15s
          ports:
            - name: https
              containerPort: 10250
              protocol: TCP
          livenessProbe:
            httpGet:
              path: /livez
              port: https
              scheme: HTTPS
            initialDelaySeconds: 10
            periodSeconds: 10
          readinessProbe:
            httpGet:
              path: /readyz
              port: https
              scheme: HTTPS
            initialDelaySeconds: 20
            periodSeconds: 10
          securityContext:
            allowPrivilegeEscalation: false
            readOnlyRootFilesystem: true
            runAsNonRoot: true
            runAsUser: 1000
          volumeMounts:
            - name: tmp
              mountPath: /tmp
      volumes:
        - name: tmp
          emptyDir: {}
---
apiVersion: apiregistration.k8s.io/v1
kind: APIService
metadata:
  name: v1beta1.metrics.k8s.io
  labels:
    k8s-app: metrics-server
    {{- include "deployment.labels" . | nindent 4 }}
spec:
  service:
    name: metrics-server
    namespace: kube-system
  group: metrics.k8s.io
  version: v1beta1
  insecureSkipTLSVerify: true
  groupPriorityMinimum: 100
  versionPriority: 100
{{- end }}
"#
    .to_string()
}
