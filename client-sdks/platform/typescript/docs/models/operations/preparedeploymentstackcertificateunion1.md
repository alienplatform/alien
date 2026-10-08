# PrepareDeploymentStackCertificateUnion1

Certificate publication or reference mode for Kubernetes public endpoints.


## Supported Types

### `operations.PrepareDeploymentStackCertificateTLSSecretRef1`

```typescript
const value: operations.PrepareDeploymentStackCertificateTLSSecretRef1 = {
  secretName: "<value>",
  mode: "tlsSecretRef",
};
```

### `operations.PrepareDeploymentStackCertificateManagedAcmImport1`

```typescript
const value: operations.PrepareDeploymentStackCertificateManagedAcmImport1 = {
  mode: "managedAcmImport",
};
```

### `operations.PrepareDeploymentStackCertificateAwsAcmArn1`

```typescript
const value: operations.PrepareDeploymentStackCertificateAwsAcmArn1 = {
  certificateArn: "<value>",
  mode: "awsAcmArn",
};
```

### `operations.PrepareDeploymentStackCertificateManagedTLSSecret1`

```typescript
const value: operations.PrepareDeploymentStackCertificateManagedTLSSecret1 = {
  mode: "managedTlsSecret",
  secretNameTemplate: "<value>",
};
```

### `operations.PrepareDeploymentStackCertificateNone1`

```typescript
const value: operations.PrepareDeploymentStackCertificateNone1 = {
  mode: "none",
};
```

