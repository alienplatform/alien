# PlanDeploymentComputeExposureUnion


## Supported Types

### `operations.PlanDeploymentComputeExposureDisabled`

```typescript
const value: operations.PlanDeploymentComputeExposureDisabled = {
  mode: "disabled",
};
```

### `operations.PlanDeploymentComputeExposureGenerated`

```typescript
const value: operations.PlanDeploymentComputeExposureGenerated = {
  certificate: {
    mode: "managedTlsSecret",
    secretNameTemplate: "<value>",
  },
  mode: "generated",
  route: {
    gatewayClassName: "<value>",
    listenerPort: 163968,
    routeApi: "gateway",
  },
};
```

### `operations.PlanDeploymentComputeExposureCustom`

```typescript
const value: operations.PlanDeploymentComputeExposureCustom = {
  certificate: {
    mode: "managedTlsSecret",
    secretNameTemplate: "<value>",
  },
  domain: "unsightly-guard.info",
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

