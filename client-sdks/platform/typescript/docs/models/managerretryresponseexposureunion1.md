# ManagerRetryResponseExposureUnion1


## Supported Types

### `models.ManagerRetryResponseExposureDisabled1`

```typescript
const value: models.ManagerRetryResponseExposureDisabled1 = {
  mode: "disabled",
};
```

### `models.ManagerRetryResponseExposureGenerated1`

```typescript
const value: models.ManagerRetryResponseExposureGenerated1 = {
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

### `models.ManagerRetryResponseExposureCustom1`

```typescript
const value: models.ManagerRetryResponseExposureCustom1 = {
  certificate: {
    secretName: "<value>",
    mode: "tlsSecretRef",
  },
  domain: "ironclad-appliance.info",
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

