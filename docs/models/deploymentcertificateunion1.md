# DeploymentCertificateUnion1

Certificate publication or reference mode for Kubernetes public endpoints.


## Supported Types

### `models.DeploymentCertificateTLSSecretRef1`

```typescript
const value: models.DeploymentCertificateTLSSecretRef1 = {
  secretName: "<value>",
  mode: "tlsSecretRef",
};
```

### `models.DeploymentCertificateManagedAcmImport1`

```typescript
const value: models.DeploymentCertificateManagedAcmImport1 = {
  mode: "managedAcmImport",
};
```

### `models.DeploymentCertificateAwsAcmArn1`

```typescript
const value: models.DeploymentCertificateAwsAcmArn1 = {
  certificateArn: "<value>",
  mode: "awsAcmArn",
};
```

### `models.DeploymentCertificateManagedTLSSecret1`

```typescript
const value: models.DeploymentCertificateManagedTLSSecret1 = {
  mode: "managedTlsSecret",
  secretNameTemplate: "<value>",
};
```

### `models.DeploymentCertificateNone1`

```typescript
const value: models.DeploymentCertificateNone1 = {
  mode: "none",
};
```

