# CreateManagerResponseExposureUnion1


## Supported Types

### `models.CreateManagerResponseExposureDisabled1`

```typescript
const value: models.CreateManagerResponseExposureDisabled1 = {
  mode: "disabled",
};
```

### `models.CreateManagerResponseExposureGenerated1`

```typescript
const value: models.CreateManagerResponseExposureGenerated1 = {
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

### `models.CreateManagerResponseExposureCustom1`

```typescript
const value: models.CreateManagerResponseExposureCustom1 = {
  certificate: {
    mode: "managedTlsSecret",
    secretNameTemplate: "<value>",
  },
  domain: "fatal-hutch.org",
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

