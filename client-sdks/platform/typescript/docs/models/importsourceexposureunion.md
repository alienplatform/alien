# ImportSourceExposureUnion


## Supported Types

### `models.ImportSourceExposureDisabled`

```typescript
const value: models.ImportSourceExposureDisabled = {
  mode: "disabled",
};
```

### `models.ImportSourceExposureGenerated`

```typescript
const value: models.ImportSourceExposureGenerated = {
  certificate: {
    secretName: "<value>",
    mode: "tlsSecretRef",
  },
  mode: "generated",
  route: {
    gatewayClassName: "<value>",
    listenerPort: 728426,
    routeApi: "gateway",
  },
};
```

### `models.ImportSourceExposureCustom`

```typescript
const value: models.ImportSourceExposureCustom = {
  certificate: {
    mode: "managedAcmImport",
  },
  domain: "smart-statue.com",
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

