# DeploymentInfoExposureUnion


## Supported Types

### `models.DeploymentInfoExposureDisabled`

```typescript
const value: models.DeploymentInfoExposureDisabled = {
  mode: "disabled",
};
```

### `models.DeploymentInfoExposureGenerated`

```typescript
const value: models.DeploymentInfoExposureGenerated = {
  certificate: {
    mode: "managedAcmImport",
  },
  mode: "generated",
  route: {
    gatewayClassName: "<value>",
    listenerPort: 316835,
    routeApi: "gateway",
  },
};
```

### `models.DeploymentInfoExposureCustom`

```typescript
const value: models.DeploymentInfoExposureCustom = {
  certificate: {
    mode: "managedTlsSecret",
    secretNameTemplate: "<value>",
  },
  domain: "ultimate-cap.org",
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

