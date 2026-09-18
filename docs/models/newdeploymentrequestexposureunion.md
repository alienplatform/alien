# NewDeploymentRequestExposureUnion


## Supported Types

### `models.NewDeploymentRequestExposureDisabled`

```typescript
const value: models.NewDeploymentRequestExposureDisabled = {
  mode: "disabled",
};
```

### `models.NewDeploymentRequestExposureGenerated`

```typescript
const value: models.NewDeploymentRequestExposureGenerated = {
  certificate: {
    mode: "managedAcmImport",
  },
  mode: "generated",
  route: {
    gatewayClassName: "<value>",
    listenerPort: 134192,
    routeApi: "gateway",
  },
};
```

### `models.NewDeploymentRequestExposureCustom`

```typescript
const value: models.NewDeploymentRequestExposureCustom = {
  certificate: {
    mode: "none",
  },
  domain: "hasty-quinoa.biz",
  mode: "custom",
  route: {
    gatewayClassName: "<value>",
    listenerPort: 622555,
    routeApi: "gateway",
  },
};
```

### `any`

```typescript
const value: any = "<value>";
```

