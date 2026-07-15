# ClientConfigUnion1

Configuration mode for Kubernetes access


## Supported Types

### `models.ClientConfigInCluster`

```typescript
const value: models.ClientConfigInCluster = {
  mode: "inCluster",
  platform: "kubernetes",
};
```

### `models.ClientConfigKubeconfig`

```typescript
const value: models.ClientConfigKubeconfig = {
  mode: "kubeconfig",
  platform: "kubernetes",
};
```

### `models.ClientConfigManual`

```typescript
const value: models.ClientConfigManual = {
  additionalHeaders: {
    "key": "<value>",
    "key1": "<value>",
  },
  mode: "manual",
  serverUrl: "https://grounded-embossing.info",
  platform: "kubernetes",
};
```

