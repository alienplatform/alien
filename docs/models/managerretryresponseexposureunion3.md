# ManagerRetryResponseExposureUnion3


## Supported Types

### `models.ManagerRetryResponseExposureDisabled3`

```typescript
const value: models.ManagerRetryResponseExposureDisabled3 = {
  mode: "disabled",
};
```

### `models.ManagerRetryResponseExposureGenerated3`

```typescript
const value: models.ManagerRetryResponseExposureGenerated3 = {
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

### `models.ManagerRetryResponseExposureCustom3`

```typescript
const value: models.ManagerRetryResponseExposureCustom3 = {
  certificate: {
    certificateArn: "<value>",
    mode: "awsAcmArn",
  },
  domain: "unconscious-begonia.name",
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

