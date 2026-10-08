# NewDeploymentRequestCertificateUnion1

Certificate publication or reference mode for Kubernetes public endpoints.


## Supported Types

### `models.NewDeploymentRequestCertificateTLSSecretRef1`

```typescript
const value: models.NewDeploymentRequestCertificateTLSSecretRef1 = {
  secretName: "<value>",
  mode: "tlsSecretRef",
};
```

### `models.NewDeploymentRequestCertificateManagedAcmImport1`

```typescript
const value: models.NewDeploymentRequestCertificateManagedAcmImport1 = {
  mode: "managedAcmImport",
};
```

### `models.NewDeploymentRequestCertificateAwsAcmArn1`

```typescript
const value: models.NewDeploymentRequestCertificateAwsAcmArn1 = {
  certificateArn: "<value>",
  mode: "awsAcmArn",
};
```

### `models.NewDeploymentRequestCertificateManagedTLSSecret1`

```typescript
const value: models.NewDeploymentRequestCertificateManagedTLSSecret1 = {
  mode: "managedTlsSecret",
  secretNameTemplate: "<value>",
};
```

### `models.NewDeploymentRequestCertificateNone1`

```typescript
const value: models.NewDeploymentRequestCertificateNone1 = {
  mode: "none",
};
```

