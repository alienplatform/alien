# PlanDeploymentComputeCertificateUnion1

Certificate publication or reference mode for Kubernetes public endpoints.


## Supported Types

### `operations.PlanDeploymentComputeCertificateTLSSecretRef1`

```typescript
const value: operations.PlanDeploymentComputeCertificateTLSSecretRef1 = {
  secretName: "<value>",
  mode: "tlsSecretRef",
};
```

### `operations.PlanDeploymentComputeCertificateManagedAcmImport1`

```typescript
const value: operations.PlanDeploymentComputeCertificateManagedAcmImport1 = {
  mode: "managedAcmImport",
};
```

### `operations.PlanDeploymentComputeCertificateAwsAcmArn1`

```typescript
const value: operations.PlanDeploymentComputeCertificateAwsAcmArn1 = {
  certificateArn: "<value>",
  mode: "awsAcmArn",
};
```

### `operations.PlanDeploymentComputeCertificateManagedTLSSecret1`

```typescript
const value: operations.PlanDeploymentComputeCertificateManagedTLSSecret1 = {
  mode: "managedTlsSecret",
  secretNameTemplate: "<value>",
};
```

### `operations.PlanDeploymentComputeCertificateNone1`

```typescript
const value: operations.PlanDeploymentComputeCertificateNone1 = {
  mode: "none",
};
```

