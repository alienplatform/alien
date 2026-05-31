# ManagerRetryResponseExposureUnion2


## Supported Types

### `models.ManagerRetryResponseExposureDisabled2`

```typescript
const value: models.ManagerRetryResponseExposureDisabled2 = {
  mode: "disabled",
};
```

### `models.ManagerRetryResponseExposureGenerated2`

```typescript
const value: models.ManagerRetryResponseExposureGenerated2 = {
  certificate: {
    mode: "managedAcmImport",
  },
  mode: "generated",
  route: {
    ingressClassName: "<value>",
    routeApi: "ingress",
  },
};
```

### `models.ManagerRetryResponseExposureCustom2`

```typescript
const value: models.ManagerRetryResponseExposureCustom2 = {
  certificate: {
    mode: "none",
  },
  domain: "immediate-phrase.info",
  mode: "custom",
  route: {
    gatewayClassName: "<value>",
    listenerPort: 665226,
    routeApi: "gateway",
  },
};
```

### `any`

```typescript
const value: any = "<value>";
```

