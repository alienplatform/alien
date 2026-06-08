# CreateSetupRegistrationOperationRequestExposureUnion


## Supported Types

### `models.CreateSetupRegistrationOperationRequestExposureDisabled`

```typescript
const value: models.CreateSetupRegistrationOperationRequestExposureDisabled = {
  mode: "disabled",
};
```

### `models.CreateSetupRegistrationOperationRequestExposureGenerated`

```typescript
const value: models.CreateSetupRegistrationOperationRequestExposureGenerated = {
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

### `models.CreateSetupRegistrationOperationRequestExposureCustom`

```typescript
const value: models.CreateSetupRegistrationOperationRequestExposureCustom = {
  certificate: {
    certificateArn: "<value>",
    mode: "awsAcmArn",
  },
  domain: "snappy-petal.info",
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

