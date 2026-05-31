# SyncAcquireResponseExposureUnion


## Supported Types

### `models.SyncAcquireResponseExposureDisabled`

```typescript
const value: models.SyncAcquireResponseExposureDisabled = {
  mode: "disabled",
};
```

### `models.SyncAcquireResponseExposureGenerated`

```typescript
const value: models.SyncAcquireResponseExposureGenerated = {
  certificate: {
    mode: "managedAcmImport",
  },
  mode: "generated",
  route: {
    gatewayClassName: "<value>",
    listenerPort: 168781,
    routeApi: "gateway",
  },
};
```

### `models.SyncAcquireResponseExposureCustom`

```typescript
const value: models.SyncAcquireResponseExposureCustom = {
  certificate: {
    secretName: "<value>",
    mode: "tlsSecretRef",
  },
  domain: "flickering-rule.com",
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

