# CreateManagerResponseExposureUnion2


## Supported Types

### `models.CreateManagerResponseExposureDisabled2`

```typescript
const value: models.CreateManagerResponseExposureDisabled2 = {
  mode: "disabled",
};
```

### `models.CreateManagerResponseExposureGenerated2`

```typescript
const value: models.CreateManagerResponseExposureGenerated2 = {
  certificate: {
    secretName: "<value>",
    mode: "tlsSecretRef",
  },
  mode: "generated",
  route: {
    gatewayClassName: "<value>",
    listenerPort: 938185,
    routeApi: "gateway",
  },
};
```

### `models.CreateManagerResponseExposureCustom2`

```typescript
const value: models.CreateManagerResponseExposureCustom2 = {
  certificate: {
    mode: "none",
  },
  domain: "baggy-verve.biz",
  mode: "custom",
  route: {
    gatewayClassName: "<value>",
    listenerPort: 581113,
    routeApi: "gateway",
  },
};
```

### `any`

```typescript
const value: any = "<value>";
```

