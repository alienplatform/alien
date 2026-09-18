# DeploymentInfoCertificateUnion2

Certificate publication or reference mode for Kubernetes public endpoints.


## Supported Types

### `models.DeploymentInfoCertificateTLSSecretRef2`

```typescript
const value: models.DeploymentInfoCertificateTLSSecretRef2 = {
  secretName: "<value>",
  mode: "tlsSecretRef",
};
```

### `models.DeploymentInfoCertificateManagedAcmImport2`

```typescript
const value: models.DeploymentInfoCertificateManagedAcmImport2 = {
  mode: "managedAcmImport",
};
```

### `models.DeploymentInfoCertificateAwsAcmArn2`

```typescript
const value: models.DeploymentInfoCertificateAwsAcmArn2 = {
  certificateArn: "<value>",
  mode: "awsAcmArn",
};
```

### `models.DeploymentInfoCertificateManagedTLSSecret2`

```typescript
const value: models.DeploymentInfoCertificateManagedTLSSecret2 = {
  mode: "managedTlsSecret",
  secretNameTemplate: "<value>",
};
```

### `models.DeploymentInfoCertificateNone2`

```typescript
const value: models.DeploymentInfoCertificateNone2 = {
  mode: "none",
};
```

