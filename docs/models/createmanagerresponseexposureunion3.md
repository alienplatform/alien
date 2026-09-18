# CreateManagerResponseExposureUnion3


## Supported Types

### `models.CreateManagerResponseExposureDisabled3`

```typescript
const value: models.CreateManagerResponseExposureDisabled3 = {
  mode: "disabled",
};
```

### `models.CreateManagerResponseExposureGenerated3`

```typescript
const value: models.CreateManagerResponseExposureGenerated3 = {
  certificate: {
    secretName: "<value>",
    mode: "tlsSecretRef",
  },
  mode: "generated",
  route: {
    ingressClassName: "<value>",
    routeApi: "ingress",
  },
};
```

### `models.CreateManagerResponseExposureCustom3`

```typescript
const value: models.CreateManagerResponseExposureCustom3 = {
  certificate: {
    mode: "managedTlsSecret",
    secretNameTemplate: "<value>",
  },
  domain: "spirited-alb.com",
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

