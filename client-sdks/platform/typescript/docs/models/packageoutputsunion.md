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
    sourceAgentBinarySha256: "<value>",
    sourceCliBinarySha256: "<value>",
  },
  type: "cli",
};
```

### `models.OutputsAgentImage`

```typescript
const value: models.OutputsAgentImage = {
  digest: "<value>",
  image: "https://loremflickr.com/2093/3847?lock=4569584363340966",
  type: "agent-image",
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

### `any`

```typescript
const value: any = "<value>";
```

