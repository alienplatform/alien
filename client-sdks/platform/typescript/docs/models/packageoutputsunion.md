# PackageOutputsUnion

Package outputs (only when status is 'ready')


## Supported Types

### `models.OutputsCli`

```typescript
const value: models.OutputsCli = {
  binaries: {},
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

