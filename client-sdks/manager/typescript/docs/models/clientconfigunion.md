# ClientConfigUnion

Configuration for different cloud platform clients


## Supported Types

### `models.ClientConfigAws`

```typescript
const value: models.ClientConfigAws = {
  accountId: "<id>",
  credentials: {
    accessKeyId: "<id>",
    secretAccessKey: "<value>",
    type: "accessKeys",
  },
  region: "<value>",
  platform: "aws",
};
```

### `models.ClientConfigGcp`

```typescript
const value: models.ClientConfigGcp = {
  credentials: {
    config: {
      scopes: [
        "<value 1>",
      ],
      serviceAccountEmail: "<value>",
    },
    source: {},
    type: "impersonatedServiceAccount",
  },
  projectId: "<id>",
  region: "<value>",
  platform: "gcp",
};
```

### `models.ClientConfigAzure`

```typescript
const value: models.ClientConfigAzure = {
  credentials: {
    tokens: {
      "key": "<value>",
      "key1": "<value>",
    },
    type: "scopedAccessTokens",
  },
  subscriptionId: "<id>",
  tenantId: "<id>",
  platform: "azure",
};
```

### `models.ClientConfigUnion1`

```typescript
const value: models.ClientConfigUnion1 = {
  additionalHeaders: {},
  mode: "manual",
  serverUrl: "https://gruesome-doing.name/",
  platform: "kubernetes",
};
```

### `models.ClientConfigKubernetesCloud`

```typescript
const value: models.ClientConfigKubernetesCloud = {
  cloud: {},
  kubernetes: {
    mode: "kubeconfig",
  },
  platform: "kubernetesCloud",
};
```

### `models.ClientConfigLocal`

```typescript
const value: models.ClientConfigLocal = {
  platform: "local",
  stateDirectory: "<value>",
};
```

