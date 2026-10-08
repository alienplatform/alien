# PrepareDeploymentStackCertificateUnion2

Certificate publication or reference mode for Kubernetes public endpoints.


## Supported Types

### `operations.PrepareDeploymentStackCertificateTLSSecretRef2`

```typescript
const value: operations.PrepareDeploymentStackCertificateTLSSecretRef2 = {
  secretName: "<value>",
  mode: "tlsSecretRef",
};
```

### `operations.PrepareDeploymentStackCertificateManagedAcmImport2`

```typescript
const value: operations.PrepareDeploymentStackCertificateManagedAcmImport2 = {
  mode: "managedAcmImport",
};
```

### `operations.PrepareDeploymentStackCertificateAwsAcmArn2`

```typescript
const value: operations.PrepareDeploymentStackCertificateAwsAcmArn2 = {
  certificateArn: "<value>",
  mode: "awsAcmArn",
};
```

### `operations.PrepareDeploymentStackCertificateManagedTLSSecret2`

```typescript
const value: operations.PrepareDeploymentStackCertificateManagedTLSSecret2 = {
  mode: "managedTlsSecret",
  secretNameTemplate: "<value>",
};
```

### `operations.PrepareDeploymentStackCertificateNone2`

```typescript
const value: operations.PrepareDeploymentStackCertificateNone2 = {
  mode: "none",
};
```

