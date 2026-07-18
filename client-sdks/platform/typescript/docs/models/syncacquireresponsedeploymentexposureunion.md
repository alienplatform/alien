# SyncAcquireResponseDeploymentExposureUnion


## Supported Types

### `models.SyncAcquireResponseDeploymentExposureDisabled`

```typescript
const value: models.SyncAcquireResponseDeploymentExposureDisabled = {
  mode: "disabled",
};
```

### `models.SyncAcquireResponseDeploymentExposureGenerated`

```typescript
const value: models.SyncAcquireResponseDeploymentExposureGenerated = {
  certificate: {
    mode: "managedTlsSecret",
    secretNameTemplate: "<value>",
  },
  mode: "generated",
  route: {
    gatewayClassName: "<value>",
    listenerPort: 312942,
    routeApi: "gateway",
  },
};
```

### `models.SyncAcquireResponseDeploymentExposureCustom`

```typescript
const value: models.SyncAcquireResponseDeploymentExposureCustom = {
  certificate: {
    mode: "managedTlsSecret",
    secretNameTemplate: "<value>",
  },
  domain: "royal-innovation.com",
  mode: "custom",
  route: {
    gatewayClassName: "<value>",
    listenerPort: 829658,
    routeApi: "gateway",
  },
};
```

### `any`

```typescript
const value: any = "<value>";
```

