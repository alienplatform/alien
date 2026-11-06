# CreateManagerResponseExposure1


## Supported Types

### `models.KubernetesExposureDisabled`

```typescript
const value: models.KubernetesExposureDisabled = {
  mode: "disabled",
};
```

### `models.KubernetesExposureGenerated`

```typescript
const value: models.KubernetesExposureGenerated = {
  certificate: {
    mode: "none",
  },
  mode: "generated",
  route: {
    gatewayClassName: "<value>",
    listenerPort: 166428,
    routeApi: "gateway",
  },
};
```

### `models.KubernetesExposureCustom`

```typescript
const value: models.KubernetesExposureCustom = {
  certificate: {
    secretName: "<value>",
    mode: "tlsSecretRef",
  },
  domain: "fearless-hammock.com",
  mode: "custom",
  route: {
    ingressClassName: "<value>",
    routeApi: "ingress",
  },
};
```

### `models.KubernetesExposureCustomBaseDomain`

```typescript
const value: models.KubernetesExposureCustomBaseDomain = {
  baseDomain: "<value>",
  certificate: {
    certificateArn: "<value>",
    mode: "awsAcmArn",
  },
  mode: "customBaseDomain",
  route: {
    gatewayClassName: "<value>",
    listenerPort: 262621,
    routeApi: "gateway",
  },
};
```

### `string`

```typescript
const value: string = "<value>";
```

