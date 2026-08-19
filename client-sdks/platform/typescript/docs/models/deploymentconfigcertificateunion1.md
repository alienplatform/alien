# DeploymentConfigCertificateUnion1

Certificate publication or reference mode for Kubernetes public endpoints.


## Supported Types

### `models.DeploymentConfigCertificateTLSSecretRef1`

```typescript
const value: models.DeploymentConfigCertificateTLSSecretRef1 = {
  secretName: "<value>",
  mode: "tlsSecretRef",
};
```

### `models.DeploymentConfigCertificateManagedAcmImport1`

```typescript
const value: models.DeploymentConfigCertificateManagedAcmImport1 = {
  mode: "managedAcmImport",
};
```

### `models.DeploymentConfigCertificateAwsAcmArn1`

```typescript
const value: models.DeploymentConfigCertificateAwsAcmArn1 = {
  certificateArn: "<value>",
  mode: "awsAcmArn",
};
```

### `models.DeploymentConfigCertificateManagedTLSSecret1`

```typescript
const value: models.DeploymentConfigCertificateManagedTLSSecret1 = {
  mode: "managedTlsSecret",
  secretNameTemplate: "<value>",
};
```

### `models.DeploymentConfigCertificateNone1`

```typescript
const value: models.DeploymentConfigCertificateNone1 = {
  mode: "none",
};
```

