# DeploymentSetupStackSettingsPolicyExposureUnion


## Supported Types

### `models.DeploymentSetupStackSettingsPolicyExposureDisabled`

```typescript
const value: models.DeploymentSetupStackSettingsPolicyExposureDisabled = {
  mode: "disabled",
};
```

### `models.DeploymentSetupStackSettingsPolicyExposureGenerated`

```typescript
const value: models.DeploymentSetupStackSettingsPolicyExposureGenerated = {
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

### `models.DeploymentSetupStackSettingsPolicyExposureCustom`

```typescript
const value: models.DeploymentSetupStackSettingsPolicyExposureCustom = {
  certificate: {
    secretName: "<value>",
    mode: "tlsSecretRef",
  },
  domain: "round-fog.com",
  mode: "custom",
  route: {
    gatewayClassName: "<value>",
    listenerPort: 53529,
    routeApi: "gateway",
  },
};
```

### `any`

```typescript
const value: any = "<value>";
```

