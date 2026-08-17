# DeploymentConfigExposureUnion


## Supported Types

### `models.DeploymentConfigExposureDisabled`

```typescript
const value: models.DeploymentConfigExposureDisabled = {
  mode: "disabled",
};
```

### `models.DeploymentConfigExposureGenerated`

```typescript
const value: models.DeploymentConfigExposureGenerated = {
  certificate: {
    secretName: "<value>",
    mode: "tlsSecretRef",
  },
  mode: "generated",
  route: {
    ingressClassName: "<value>",
    routeApi: "ingress",
  },
};
```

### `models.DeploymentConfigExposureCustom`

```typescript
const value: models.DeploymentConfigExposureCustom = {
  certificate: {
    secretName: "<value>",
    mode: "tlsSecretRef",
  },
  domain: "ultimate-petal.net",
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

