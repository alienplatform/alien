# TargetDeploymentCertificateUnion1

Certificate publication or reference mode for Kubernetes public endpoints.


## Supported Types

### `models.TargetDeploymentCertificateTLSSecretRef1`

```typescript
const value: models.TargetDeploymentCertificateTLSSecretRef1 = {
  secretName: "<value>",
  mode: "tlsSecretRef",
};
```

### `models.TargetDeploymentCertificateManagedAcmImport1`

```typescript
const value: models.TargetDeploymentCertificateManagedAcmImport1 = {
  mode: "managedAcmImport",
};
```

### `models.TargetDeploymentCertificateAwsAcmArn1`

```typescript
const value: models.TargetDeploymentCertificateAwsAcmArn1 = {
  certificateArn: "<value>",
  mode: "awsAcmArn",
};
```

### `models.TargetDeploymentCertificateManagedTLSSecret1`

```typescript
const value: models.TargetDeploymentCertificateManagedTLSSecret1 = {
  mode: "managedTlsSecret",
  secretNameTemplate: "<value>",
};
```

### `models.TargetDeploymentCertificateNone1`

```typescript
const value: models.TargetDeploymentCertificateNone1 = {
  mode: "none",
};
```

