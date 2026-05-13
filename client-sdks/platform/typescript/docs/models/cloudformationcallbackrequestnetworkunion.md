# CloudFormationCallbackRequestNetworkUnion


## Supported Types

### `models.CloudFormationCallbackRequestNetworkUseDefault`

```typescript
const value: models.CloudFormationCallbackRequestNetworkUseDefault = {
  type: "use-default",
};
```

### `models.CloudFormationCallbackRequestNetworkCreate`

```typescript
const value: models.CloudFormationCallbackRequestNetworkCreate = {
  type: "create",
};
```

### `models.CloudFormationCallbackRequestNetworkByoVpcAws`

```typescript
const value: models.CloudFormationCallbackRequestNetworkByoVpcAws = {
  privateSubnetIds: [
    "<value 1>",
    "<value 2>",
  ],
  publicSubnetIds: [
    "<value 1>",
    "<value 2>",
  ],
  type: "byo-vpc-aws",
  vpcId: "<id>",
};
```

### `models.CloudFormationCallbackRequestNetworkByoVpcGcp`

```typescript
const value: models.CloudFormationCallbackRequestNetworkByoVpcGcp = {
  networkName: "<value>",
  region: "<value>",
  subnetName: "<value>",
  type: "byo-vpc-gcp",
};
```

### `models.CloudFormationCallbackRequestNetworkByoVnetAzure`

```typescript
const value: models.CloudFormationCallbackRequestNetworkByoVnetAzure = {
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

