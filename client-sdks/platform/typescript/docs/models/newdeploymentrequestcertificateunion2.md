# NewDeploymentRequestCertificateUnion2

Certificate publication or reference mode for Kubernetes public endpoints.


## Supported Types

### `models.NewDeploymentRequestCertificateTLSSecretRef2`

```typescript
const value: models.NewDeploymentRequestCertificateTLSSecretRef2 = {
  secretName: "<value>",
  mode: "tlsSecretRef",
};
```

### `models.NewDeploymentRequestCertificateManagedAcmImport2`

```typescript
const value: models.NewDeploymentRequestCertificateManagedAcmImport2 = {
  mode: "managedAcmImport",
};
```

### `models.NewDeploymentRequestCertificateAwsAcmArn2`

```typescript
const value: models.NewDeploymentRequestCertificateAwsAcmArn2 = {
  certificateArn: "<value>",
  mode: "awsAcmArn",
};
```

### `models.NewDeploymentRequestCertificateManagedTLSSecret2`

```typescript
const value: models.NewDeploymentRequestCertificateManagedTLSSecret2 = {
  mode: "managedTlsSecret",
  secretNameTemplate: "<value>",
};
```

### `models.NewDeploymentRequestCertificateNone2`

```typescript
const value: models.NewDeploymentRequestCertificateNone2 = {
  mode: "none",
};
```

