# SyncReconcileResponseExposureUnion


## Supported Types

### `models.SyncReconcileResponseExposureDisabled`

```typescript
const value: models.SyncReconcileResponseExposureDisabled = {
  mode: "disabled",
};
```

### `models.SyncReconcileResponseExposureGenerated`

```typescript
const value: models.SyncReconcileResponseExposureGenerated = {
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

### `models.SyncReconcileResponseExposureCustom`

```typescript
const value: models.SyncReconcileResponseExposureCustom = {
  certificate: {
    mode: "managedTlsSecret",
    secretNameTemplate: "<value>",
  },
  domain: "difficult-switchboard.com",
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

