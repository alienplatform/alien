# CloudFormationCallbackRequestExposureUnion


## Supported Types

### `models.CloudFormationCallbackRequestExposureDisabled`

```typescript
const value: models.CloudFormationCallbackRequestExposureDisabled = {
  mode: "disabled",
};
```

### `models.CloudFormationCallbackRequestExposureGenerated`

```typescript
const value: models.CloudFormationCallbackRequestExposureGenerated = {
  certificate: {
    mode: "managedAcmImport",
  },
  mode: "generated",
  route: {
    gatewayClassName: "<value>",
    listenerPort: 348169,
    routeApi: "gateway",
  },
};
```

### `models.CloudFormationCallbackRequestExposureCustom`

```typescript
const value: models.CloudFormationCallbackRequestExposureCustom = {
  certificate: {
    mode: "managedTlsSecret",
    secretNameTemplate: "<value>",
  },
  domain: "courteous-descent.com",
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

