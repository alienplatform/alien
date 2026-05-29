# DeploymentDetailResponseCertificateUnion1

Certificate publication or reference mode for Kubernetes public endpoints.


## Supported Types

### `models.DeploymentDetailResponseCertificateTLSSecretRef1`

```typescript
const value: models.DeploymentDetailResponseCertificateTLSSecretRef1 = {
  secretName: "<value>",
  mode: "tlsSecretRef",
};
```

### `models.DeploymentDetailResponseCertificateManagedAcmImport1`

```typescript
const value: models.DeploymentDetailResponseCertificateManagedAcmImport1 = {
  mode: "managedAcmImport",
};
```

### `models.DeploymentDetailResponseCertificateAwsAcmArn1`

```typescript
const value: models.DeploymentDetailResponseCertificateAwsAcmArn1 = {
  certificateArn: "<value>",
  mode: "awsAcmArn",
};
```

### `models.DeploymentDetailResponseCertificateManagedTLSSecret1`

```typescript
const value: models.DeploymentDetailResponseCertificateManagedTLSSecret1 = {
  mode: "managedTlsSecret",
  secretNameTemplate: "<value>",
};
```

### `models.DeploymentDetailResponseCertificateNone1`

```typescript
const value: models.DeploymentDetailResponseCertificateNone1 = {
  mode: "none",
};
```

