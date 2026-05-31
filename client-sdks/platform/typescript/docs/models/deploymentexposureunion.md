# DeploymentExposureUnion


## Supported Types

### `models.DeploymentExposureDisabled`

```typescript
const value: models.DeploymentExposureDisabled = {
  mode: "disabled",
};
```

### `models.DeploymentExposureGenerated`

```typescript
const value: models.DeploymentExposureGenerated = {
  certificate: {
    mode: "managedTlsSecret",
    secretNameTemplate: "<value>",
  },
  mode: "generated",
  route: {
    ingressClassName: "<value>",
    routeApi: "ingress",
  },
};
```

### `models.DeploymentExposureCustom`

```typescript
const value: models.DeploymentExposureCustom = {
  certificate: {
    mode: "managedAcmImport",
  },
  domain: "bulky-toothpick.net",
  mode: "custom",
  route: {
    ingressClassName: "<value>",
    routeApi: "ingress",
  },
};
```

### `any`

```typescript
const value: any = "<value>";
```

