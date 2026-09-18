# DeploymentDetailResponseExposureUnion


## Supported Types

### `models.DeploymentDetailResponseExposureDisabled`

```typescript
const value: models.DeploymentDetailResponseExposureDisabled = {
  mode: "disabled",
};
```

### `models.DeploymentDetailResponseExposureGenerated`

```typescript
const value: models.DeploymentDetailResponseExposureGenerated = {
  certificate: {
    mode: "managedAcmImport",
  },
  mode: "generated",
  route: {
    gatewayClassName: "<value>",
    listenerPort: 152308,
    routeApi: "gateway",
  },
};
```

### `models.DeploymentDetailResponseExposureCustom`

```typescript
const value: models.DeploymentDetailResponseExposureCustom = {
  certificate: {
    mode: "managedAcmImport",
  },
  domain: "flawed-daughter.net",
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

