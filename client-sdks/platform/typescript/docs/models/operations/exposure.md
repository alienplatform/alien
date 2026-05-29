# Exposure


## Supported Types

### `operations.ExposureDisabled`

```typescript
const value: operations.ExposureDisabled = {
  mode: "disabled",
};
```

### `operations.ExposureGenerated`

```typescript
const value: operations.ExposureGenerated = {
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

### `operations.ExposureCustom`

```typescript
const value: operations.ExposureCustom = {
  certificate: {
    mode: "managedAcmImport",
  },
  domain: "miserable-planula.info",
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

