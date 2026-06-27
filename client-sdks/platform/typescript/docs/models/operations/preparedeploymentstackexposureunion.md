# PrepareDeploymentStackExposureUnion


## Supported Types

### `operations.PrepareDeploymentStackExposureDisabled`

```typescript
const value: operations.PrepareDeploymentStackExposureDisabled = {
  mode: "disabled",
};
```

### `operations.PrepareDeploymentStackExposureGenerated`

```typescript
const value: operations.PrepareDeploymentStackExposureGenerated = {
  certificate: {
    mode: "none",
  },
  mode: "generated",
  route: {
    gatewayClassName: "<value>",
    listenerPort: 60732,
    routeApi: "gateway",
  },
};
```

### `operations.PrepareDeploymentStackExposureCustom`

```typescript
const value: operations.PrepareDeploymentStackExposureCustom = {
  certificate: {
    certificateArn: "<value>",
    mode: "awsAcmArn",
  },
  domain: "messy-chainstay.org",
  mode: "custom",
  route: {
    gatewayClassName: "<value>",
    listenerPort: 185232,
    routeApi: "gateway",
  },
};
```

### `any`

```typescript
const value: any = "<value>";
```

