# TargetDeploymentCertificateUnion2

Certificate publication or reference mode for Kubernetes public endpoints.


## Supported Types

### `models.TargetDeploymentCertificateTLSSecretRef2`

```typescript
const value: models.TargetDeploymentCertificateTLSSecretRef2 = {
  secretName: "<value>",
  mode: "tlsSecretRef",
};
```

### `models.TargetDeploymentCertificateManagedAcmImport2`

```typescript
const value: models.TargetDeploymentCertificateManagedAcmImport2 = {
  mode: "managedAcmImport",
};
```

### `models.TargetDeploymentCertificateAwsAcmArn2`

```typescript
const value: models.TargetDeploymentCertificateAwsAcmArn2 = {
  certificateArn: "<value>",
  mode: "awsAcmArn",
};
```

### `models.TargetDeploymentCertificateManagedTLSSecret2`

```typescript
const value: models.TargetDeploymentCertificateManagedTLSSecret2 = {
  mode: "managedTlsSecret",
  secretNameTemplate: "<value>",
};
```

### `models.TargetDeploymentCertificateNone2`

```typescript
const value: models.TargetDeploymentCertificateNone2 = {
  mode: "none",
};
```

