# StackSettingsExposureUnion


## Supported Types

### `models.StackSettingsExposureDisabled`

```typescript
const value: models.StackSettingsExposureDisabled = {
  mode: "disabled",
};
```

### `models.StackSettingsExposureGenerated`

```typescript
const value: models.StackSettingsExposureGenerated = {
  certificate: {
    mode: "managedTlsSecret",
    secretNameTemplate: "<value>",
  },
  mode: "generated",
  route: {
    gatewayClassName: "<value>",
    listenerPort: 584209,
    routeApi: "gateway",
  },
};
```

### `models.StackSettingsExposureCustom`

```typescript
const value: models.StackSettingsExposureCustom = {
  certificate: {
    mode: "managedTlsSecret",
    secretNameTemplate: "<value>",
  },
  domain: "gracious-compromise.info",
  mode: "custom",
  route: {
    gatewayClassName: "<value>",
    listenerPort: 122317,
    routeApi: "gateway",
  },
};
```

### `any`

```typescript
const value: any = "<value>";
```

