# PlanDeploymentComputeCertificateUnion2

Certificate publication or reference mode for Kubernetes public endpoints.


## Supported Types

### `operations.PlanDeploymentComputeCertificateTLSSecretRef2`

```typescript
const value: operations.PlanDeploymentComputeCertificateTLSSecretRef2 = {
  secretName: "<value>",
  mode: "tlsSecretRef",
};
```

### `operations.PlanDeploymentComputeCertificateManagedAcmImport2`

```typescript
const value: operations.PlanDeploymentComputeCertificateManagedAcmImport2 = {
  mode: "managedAcmImport",
};
```

### `operations.PlanDeploymentComputeCertificateAwsAcmArn2`

```typescript
const value: operations.PlanDeploymentComputeCertificateAwsAcmArn2 = {
  certificateArn: "<value>",
  mode: "awsAcmArn",
};
```

### `operations.PlanDeploymentComputeCertificateManagedTLSSecret2`

```typescript
const value: operations.PlanDeploymentComputeCertificateManagedTLSSecret2 = {
  mode: "managedTlsSecret",
  secretNameTemplate: "<value>",
};
```

### `operations.PlanDeploymentComputeCertificateNone2`

```typescript
const value: operations.PlanDeploymentComputeCertificateNone2 = {
  mode: "none",
};
```

