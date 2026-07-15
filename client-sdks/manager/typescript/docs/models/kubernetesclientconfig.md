# KubernetesClientConfig

Configuration mode for Kubernetes access


## Supported Types

### `models.KubernetesClientConfigInCluster`

```typescript
const value: models.KubernetesClientConfigInCluster = {
  mode: "inCluster",
};
```

### `models.KubernetesClientConfigKubeconfig`

```typescript
const value: models.KubernetesClientConfigKubeconfig = {
  mode: "kubeconfig",
};
```

### `models.KubernetesClientConfigManual`

```typescript
const value: models.KubernetesClientConfigManual = {
  additionalHeaders: {},
  mode: "manual",
  serverUrl: "https://buzzing-t-shirt.biz",
};
```

