# TargetDeploymentExposureUnion


## Supported Types

### `models.TargetDeploymentExposureDisabled`

```typescript
const value: models.TargetDeploymentExposureDisabled = {
  mode: "disabled",
};
```

### `models.TargetDeploymentExposureGenerated`

```typescript
const value: models.TargetDeploymentExposureGenerated = {
  certificate: {
    mode: "managedAcmImport",
  },
  mode: "generated",
  route: {
    gatewayClassName: "<value>",
    listenerPort: 396903,
    routeApi: "gateway",
  },
};
```

### `models.TargetDeploymentExposureCustom`

```typescript
const value: models.TargetDeploymentExposureCustom = {
  certificate: {
    certificateArn: "<value>",
    mode: "awsAcmArn",
  },
  domain: "grounded-ignorance.com",
  mode: "custom",
  route: {
    gatewayClassName: "<value>",
    listenerPort: 97581,
    routeApi: "gateway",
  },
};
```

### `any`

```typescript
const value: any = "<value>";
```

