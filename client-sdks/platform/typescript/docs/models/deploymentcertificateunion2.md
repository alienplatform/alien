# DeploymentCertificateUnion2

Certificate publication or reference mode for Kubernetes public endpoints.


## Supported Types

### `models.DeploymentCertificateTLSSecretRef2`

```typescript
const value: models.DeploymentCertificateTLSSecretRef2 = {
  secretName: "<value>",
  mode: "tlsSecretRef",
};
```

### `models.DeploymentCertificateManagedAcmImport2`

```typescript
const value: models.DeploymentCertificateManagedAcmImport2 = {
  mode: "managedAcmImport",
};
```

### `models.DeploymentCertificateAwsAcmArn2`

```typescript
const value: models.DeploymentCertificateAwsAcmArn2 = {
  certificateArn: "<value>",
  mode: "awsAcmArn",
};
```

### `models.DeploymentCertificateManagedTLSSecret2`

```typescript
const value: models.DeploymentCertificateManagedTLSSecret2 = {
  mode: "managedTlsSecret",
  secretNameTemplate: "<value>",
};
```

### `models.DeploymentCertificateNone2`

```typescript
const value: models.DeploymentCertificateNone2 = {
  mode: "none",
};
```

