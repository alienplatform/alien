# DeploymentConfigCertificateUnion2

Certificate publication or reference mode for Kubernetes public endpoints.


## Supported Types

### `models.DeploymentConfigCertificateTLSSecretRef2`

```typescript
const value: models.DeploymentConfigCertificateTLSSecretRef2 = {
  secretName: "<value>",
  mode: "tlsSecretRef",
};
```

### `models.DeploymentConfigCertificateManagedAcmImport2`

```typescript
const value: models.DeploymentConfigCertificateManagedAcmImport2 = {
  mode: "managedAcmImport",
};
```

### `models.DeploymentConfigCertificateAwsAcmArn2`

```typescript
const value: models.DeploymentConfigCertificateAwsAcmArn2 = {
  certificateArn: "<value>",
  mode: "awsAcmArn",
};
```

### `models.DeploymentConfigCertificateManagedTLSSecret2`

```typescript
const value: models.DeploymentConfigCertificateManagedTLSSecret2 = {
  mode: "managedTlsSecret",
  secretNameTemplate: "<value>",
};
```

### `models.DeploymentConfigCertificateNone2`

```typescript
const value: models.DeploymentConfigCertificateNone2 = {
  mode: "none",
};
```

