# PersistImportedDeploymentRequestExposureUnion


## Supported Types

### `models.PersistImportedDeploymentRequestExposureDisabled`

```typescript
const value: models.PersistImportedDeploymentRequestExposureDisabled = {
  mode: "disabled",
};
```

### `models.PersistImportedDeploymentRequestExposureGenerated`

```typescript
const value: models.PersistImportedDeploymentRequestExposureGenerated = {
  certificate: {
    mode: "none",
  },
  mode: "generated",
  route: {
    ingressClassName: "<value>",
    routeApi: "ingress",
  },
};
```

### `models.PersistImportedDeploymentRequestExposureCustom`

```typescript
const value: models.PersistImportedDeploymentRequestExposureCustom = {
  certificate: {
    secretName: "<value>",
    mode: "tlsSecretRef",
  },
  domain: "male-freckle.name",
  mode: "custom",
  route: {
    gatewayClassName: "<value>",
    listenerPort: 143133,
    routeApi: "gateway",
  },
};
```

### `any`

```typescript
const value: any = "<value>";
```

