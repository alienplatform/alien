# PackageOutputsUnion

Package outputs (only when status is 'ready')


## Supported Types

### `models.OutputsCli`

```typescript
const value: models.OutputsCli = {
  binaries: {},
  buildInfo: {
    alienSha: "<value>",
    horizonSha: "<value>",
    platformSha: "<value>",
    sourceCliBinarySha256: "<value>",
  },
  type: "cli",
};
```

### `models.OutputsOperatorImage`

```typescript
const value: models.OutputsOperatorImage = {
  digest: "<value>",
  image: "https://loremflickr.com/104/2323?lock=152100383342186",
  type: "operator-image",
};
```

### `models.OutputsHelm`

```typescript
const value: models.OutputsHelm = {
  chart: "<value>",
  version: "<value>",
  type: "helm",
};
```

### `models.OutputsCloudformation`

```typescript
const value: models.OutputsCloudformation = {
  targets: {},
  type: "cloudformation",
};
```

### `models.OutputsGcpSandboxImage`

```typescript
const value: models.OutputsGcpSandboxImage = {
  agentDigest: "<value>",
  baseDigest: "<value>",
  digest: "<value>",
  image: "https://picsum.photos/seed/IYDQUAdZ5e/827/188",
  reuseKey: "<value>",
  type: "gcp-sandbox-image",
};
```

### `models.OutputsSandboxBundle`

```typescript
const value: models.OutputsSandboxBundle = {
  bundleUriTemplate: "<value>",
  objectKey: "<value>",
  regions: [],
  sha256: "<value>",
  size: 830757,
  type: "sandbox-bundle",
};
```

### `models.OutputsTerraform`

```typescript
const value: models.OutputsTerraform = {
  modules: {},
  provider: {
    gpgPublicKey: {
      asciiArmor: "<value>",
      keyId: "<id>",
    },
    platforms: {},
    source: "<value>",
  },
  type: "terraform",
};
```

### `string`

```typescript
const value: string = "<value>";
```
