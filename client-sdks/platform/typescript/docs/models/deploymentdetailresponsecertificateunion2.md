# DeploymentDetailResponseCertificateUnion2

Certificate publication or reference mode for Kubernetes public endpoints.


## Supported Types

### `models.DeploymentDetailResponseCertificateTLSSecretRef2`

```typescript
const value: models.DeploymentDetailResponseCertificateTLSSecretRef2 = {
  secretName: "<value>",
  mode: "tlsSecretRef",
};
```

### `models.DeploymentDetailResponseCertificateManagedAcmImport2`

```typescript
const value: models.DeploymentDetailResponseCertificateManagedAcmImport2 = {
  mode: "managedAcmImport",
};
```

### `models.DeploymentDetailResponseCertificateAwsAcmArn2`

```typescript
const value: models.DeploymentDetailResponseCertificateAwsAcmArn2 = {
  certificateArn: "<value>",
  mode: "awsAcmArn",
};
```

### `models.DeploymentDetailResponseCertificateManagedTLSSecret2`

```typescript
const value: models.DeploymentDetailResponseCertificateManagedTLSSecret2 = {
  mode: "managedTlsSecret",
  secretNameTemplate: "<value>",
};
```

### `models.DeploymentDetailResponseCertificateNone2`

```typescript
const value: models.DeploymentDetailResponseCertificateNone2 = {
  mode: "none",
};
```

