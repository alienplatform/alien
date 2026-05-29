# CloudFormationCallbackRequestCertificateUnion1

Certificate publication or reference mode for Kubernetes public endpoints.


## Supported Types

### `models.CloudFormationCallbackRequestCertificateTLSSecretRef1`

```typescript
const value: models.CloudFormationCallbackRequestCertificateTLSSecretRef1 = {
  secretName: "<value>",
  mode: "tlsSecretRef",
};
```

### `models.CloudFormationCallbackRequestCertificateManagedAcmImport1`

```typescript
const value: models.CloudFormationCallbackRequestCertificateManagedAcmImport1 =
  {
    mode: "managedAcmImport",
  };
```

### `models.CloudFormationCallbackRequestCertificateAwsAcmArn1`

```typescript
const value: models.CloudFormationCallbackRequestCertificateAwsAcmArn1 = {
  certificateArn: "<value>",
  mode: "awsAcmArn",
};
```

### `models.CloudFormationCallbackRequestCertificateManagedTLSSecret1`

```typescript
const value: models.CloudFormationCallbackRequestCertificateManagedTLSSecret1 =
  {
    mode: "managedTlsSecret",
    secretNameTemplate: "<value>",
  };
```

### `models.CloudFormationCallbackRequestCertificateNone1`

```typescript
const value: models.CloudFormationCallbackRequestCertificateNone1 = {
  mode: "none",
};
```

