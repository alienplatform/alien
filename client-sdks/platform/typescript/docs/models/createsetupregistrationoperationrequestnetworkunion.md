# CreateSetupRegistrationOperationRequestNetworkUnion


## Supported Types

### `models.CreateSetupRegistrationOperationRequestNetworkUseDefault`

```typescript
const value: models.CreateSetupRegistrationOperationRequestNetworkUseDefault = {
  type: "use-default",
};
```

### `models.CreateSetupRegistrationOperationRequestNetworkCreate`

```typescript
const value: models.CreateSetupRegistrationOperationRequestNetworkCreate = {
  type: "create",
};
```

### `models.CreateSetupRegistrationOperationRequestNetworkByoVpcAws`

```typescript
const value: models.CreateSetupRegistrationOperationRequestNetworkByoVpcAws = {
  privateSubnetIds: [
    "<value 1>",
    "<value 2>",
  ],
  publicSubnetIds: [
    "<value 1>",
    "<value 2>",
    "<value 3>",
  ],
  type: "byo-vpc-aws",
  vpcId: "<id>",
};
```

### `models.CreateSetupRegistrationOperationRequestNetworkByoVpcGcp`

```typescript
const value: models.CreateSetupRegistrationOperationRequestNetworkByoVpcGcp = {
  networkName: "<value>",
  region: "<value>",
  subnetName: "<value>",
  type: "byo-vpc-gcp",
};
```

### `models.CreateSetupRegistrationOperationRequestNetworkByoVnetAzure`

```typescript
const value: models.CreateSetupRegistrationOperationRequestNetworkByoVnetAzure =
  {
    privateSubnetName: "<value>",
    publicSubnetName: "<value>",
    type: "byo-vnet-azure",
    vnetResourceId: "<id>",
  };
```

### `any`

```typescript
const value: any = "<value>";
```

