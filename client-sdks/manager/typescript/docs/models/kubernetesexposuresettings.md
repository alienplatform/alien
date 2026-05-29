# KubernetesExposureSettings

Kubernetes public HTTPS exposure mode.


## Supported Types

### `models.KubernetesExposureSettingsDisabled`

```typescript
const value: models.KubernetesExposureSettingsDisabled = {
  mode: "disabled",
};
```

### `models.KubernetesExposureSettingsGenerated`

```typescript
const value: models.KubernetesExposureSettingsGenerated = {
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

### `models.KubernetesExposureSettingsCustom`

```typescript
const value: models.KubernetesExposureSettingsCustom = {
  certificate: {
    mode: "none",
  },
  domain: "quick-witted-wallaby.org",
  mode: "custom",
  route: {
    gatewayClassName: "<value>",
    listenerPort: 466794,
    routeApi: "gateway",
  },
};
```

