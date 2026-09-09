# DeploymentInfoCertificateUnion1

Certificate publication or reference mode for Kubernetes public endpoints.


## Supported Types

### `models.DeploymentInfoCertificateTLSSecretRef1`

```typescript
const value: models.DeploymentInfoCertificateTLSSecretRef1 = {
  secretName: "<value>",
  mode: "tlsSecretRef",
};
```

### `models.DeploymentInfoCertificateManagedAcmImport1`

```typescript
const value: models.DeploymentInfoCertificateManagedAcmImport1 = {
  mode: "managedAcmImport",
};
```

### `models.DeploymentInfoCertificateAwsAcmArn1`

```typescript
const value: models.DeploymentInfoCertificateAwsAcmArn1 = {
  certificateArn: "<value>",
  mode: "awsAcmArn",
};
```

### `models.DeploymentInfoCertificateManagedTLSSecret1`

```typescript
const value: models.DeploymentInfoCertificateManagedTLSSecret1 = {
  mode: "managedTlsSecret",
  secretNameTemplate: "<value>",
};
```

### `models.DeploymentInfoCertificateNone1`

```typescript
const value: models.DeploymentInfoCertificateNone1 = {
  mode: "none",
};
```

