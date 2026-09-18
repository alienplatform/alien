# SyncListResponseExposureUnion


## Supported Types

### `models.SyncListResponseExposureDisabled`

```typescript
const value: models.SyncListResponseExposureDisabled = {
  mode: "disabled",
};
```

### `models.SyncListResponseExposureGenerated`

```typescript
const value: models.SyncListResponseExposureGenerated = {
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

### `models.SyncListResponseExposureCustom`

```typescript
const value: models.SyncListResponseExposureCustom = {
  certificate: {
    certificateArn: "<value>",
    mode: "awsAcmArn",
  },
  domain: "general-dish.name",
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

