"""Native AzureRM import and alias removal using the generated policy configuration.

Only localhost is used. The ARM model implements GET/PUT/DELETE policy semantics;
it does not claim Azure propagation or lifecycle execution qualification.
"""
import base64
import copy
import http.server
import json
import os
from pathlib import Path
import ssl
import subprocess
import sys
import threading
import urllib.parse

cli = os.environ.get('TF_CLI', 'terraform')
assert cli in ('terraform', 'tofu')
root = Path(sys.argv[1])
subscription = '00000000-0000-0000-0000-000000000001'
tenant = '00000000-0000-0000-0000-000000000002'
account = f'/subscriptions/{subscription}/resourceGroups/fixture/providers/Microsoft.Storage/storageAccounts/fixtureaccount'
identity = account + '/managementPolicies/default'
remote = None
lose_update_response = False
writes = []
unexpected = []

class Handler(http.server.BaseHTTPRequestHandler):
    def log_message(self, *args):
        pass

    def reply(self, status, data):
        self.send_response(status)
        self.send_header('Content-Type', 'application/json')
        self.end_headers()
        self.wfile.write(json.dumps(data).encode())

    def do_GET(self):
        path = urllib.parse.urlsplit(self.path).path
        if path == '/metadata/endpoints':
            self.reply(200, {'name': 'AzureCloud', 'resourceManager': endpoint,
                            'microsoftGraphResourceId': endpoint,
                            'authentication': {'loginEndpoint': endpoint, 'audiences': [endpoint],
                                               'tenant': 'common', 'identityProvider': 'AAD'},
                            'suffixes': {'storage': 'localhost', 'keyVaultDns': 'localhost'}})
        elif path == '/token':
            def enc(value):
                return base64.urlsafe_b64encode(json.dumps(value).encode()).decode().rstrip('=')
            token = enc({'alg': 'none'}) + '.' + enc({'tid': tenant, 'oid': tenant, 'appid': tenant,
                                                     'exp': 4102444800}) + '.synthetic'
            self.reply(200, {'access_token': token, 'token_type': 'Bearer', 'expires_in': '3600'})
        elif path == identity:
            self.reply(200, remote) if remote else self.reply(404, {'error': {'code': 'ResourceNotFound', 'message': 'absent'}})
        else:
            unexpected.append(['GET', path])
            self.reply(400, {'error': {'code': 'Unexpected', 'message': path}})

    def do_PUT(self):
        global remote, lose_update_response
        path = urllib.parse.urlsplit(self.path).path
        if path != identity:
            unexpected.append(['PUT', path])
            self.reply(400, {'error': {'code': 'Unexpected', 'message': path}})
            return
        payload = json.loads(self.rfile.read(int(self.headers['Content-Length'])))
        writes.append('PUT')
        remote = payload | {'id': identity, 'name': 'default', 'type': 'Microsoft.Storage/storageAccounts/managementPolicies'}
        if lose_update_response:
            lose_update_response = False
            self.reply(403, {'error': {'code': 'AuthorizationFailed', 'message': 'synthetic response failure after PUT'}})
        else:
            self.reply(200, remote)

    def do_DELETE(self):
        writes.append('DELETE')
        self.reply(400, {'error': {'code': 'UnexpectedDelete', 'message': 'shared policy must survive'}})

server = http.server.ThreadingHTTPServer(('127.0.0.1', 0), Handler)
endpoint = f'https://localhost:{server.server_port}'
subprocess.run(['openssl', 'req', '-x509', '-newkey', 'rsa:2048', '-nodes',
                '-keyout', str(root / 'key.pem'), '-out', str(root / 'cert.pem'),
                '-days', '1', '-subj', '/CN=localhost', '-addext', 'subjectAltName=DNS:localhost'],
               check=True, capture_output=True)
context = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
context.load_cert_chain(root / 'cert.pem', root / 'key.pem')
server.socket = context.wrap_socket(server.socket, server_side=True)
threading.Thread(target=server.serve_forever, daemon=True).start()
env = os.environ | {'SSL_CERT_FILE': str(root / 'cert.pem'), 'TF_IN_AUTOMATION': '1',
                    'ARM_PROVIDER_ENHANCED_VALIDATION': 'false'}
(root / 'provider.tf').write_text(f'''
terraform {{
  required_providers {{
    azurerm = {{ source = "registry.terraform.io/hashicorp/azurerm", version = ">= 4.75, < 5.0" }}
  }}
}}
provider "azurerm" {{
  features {{}}
  subscription_id = "{subscription}"
  tenant_id = "{tenant}"
  metadata_host = "localhost:{server.server_port}"
  use_msi = true
  use_cli = false
  msi_endpoint = "{endpoint}/token"
  resource_provider_registrations = "none"
}}
locals {{
  account_id = "{account}"
  resource_prefix = "migration-test"
}}
resource "terraform_data" "retained" {{ input = "retained-object" }}
resource "terraform_data" "retention_inventory" {{ input = {{version=1, addresses=["terraform_data.retained"]}} }}
variable "azure_storage_import_existing_policy" {{ default = true }}
variable "azure_storage_lifecycle_enabled" {{ default = true }}
variable "azure_storage_live_lifecycle_enabled" {{ default = false }}
variable "input_files" {{ default = true }}
''')
old = '''resource "azurerm_storage_management_policy" "archive" {
  storage_account_id = local.account_id
  rule {
    name = "archive-rule-1"
    enabled = true
    filters {
      blob_types = ["blockBlob"]
      prefix_match = ["migration-test-archive/"]
    }
    actions {
      base_blob { delete_after_days_since_modification_greater_than = 30 }
    }
  }
}
'''
(root / 'main.tf').write_text(old)
(root / 'retained-data.json').write_text('["terraform_data.retained"]')

def run(*args, ok=True):
    if args[0] == 'terraform':
        args = (cli, *args[1:])
    result = subprocess.run(args, cwd=root, env=env, capture_output=True, text=True, timeout=120)
    if ok and result.returncode:
        raise AssertionError(result.stdout + result.stderr)
    if not ok:
        assert result.returncode != 0, 'unsafe workflow unexpectedly succeeded'
    return result.stdout

def tf(*args):
    return run('terraform', *args, '-no-color')

try:
    tf('init', '-input=false')
    tf('apply', '-auto-approve', '-input=false')
    assert writes == ['PUT'] and remote['id'] == identity
    state = json.loads(run('terraform', 'state', 'pull'))
    policy = next(resource for resource in state['resources'] if resource['type'] == 'azurerm_storage_management_policy')
    alias = copy.deepcopy(policy)
    alias['name'] = 'scratch'
    alias['instances'][0]['index_key'] = 0
    state['resources'].append(alias)
    state['serial'] += 1
    (root / 'aliases.json').write_text(json.dumps(state))
    run('terraform', 'state', 'push', 'aliases.json')
    (root / 'main.tf').write_text((root / 'generated-policy.txt').read_text())
    tf('plan', '-input=false', '-out=update.plan')
    plan = json.loads(run('terraform', 'show', '-json', 'update.plan'))
    policy_changes = [change for change in plan['resource_changes'] if change['type'] == 'azurerm_storage_management_policy']
    assert len(policy_changes) == 3
    for change in policy_changes:
        if change['name'] == 'default_storage_account':
            assert change['change']['importing']['id'] == identity
        else:
            assert change['change']['actions'] == ['forget']
    lose_update_response = True
    run('terraform', 'apply', '-input=false', 'update.plan', ok=False)
    assert not lose_update_response, 'the provider must have reached the modeled update'
    tf('plan', '-input=false', '-out=retry.plan')
    run('terraform', 'apply', '-input=false', 'retry.plan')
    assert remote['id'] == identity and 'DELETE' not in writes
    rules = remote['properties']['policy']['rules']
    assert len(rules) == 2
    assert {rule['name']: rule['enabled'] for rule in rules} == {'archive-rule-1': True, 'scratch-rule-1': False}
    after = json.loads(run('terraform', 'state', 'pull'))
    policies = [resource for resource in after['resources'] if resource['type'] == 'azurerm_storage_management_policy']
    assert len(policies) == 1 and policies[0]['instances'][0]['attributes']['id'] == identity
    tf('plan', '-input=false', '-detailed-exitcode')
    assert not unexpected, unexpected
    print('Native AzureRM guarded migration passed: exact ID, two aliases forgotten, no DELETE, Live expiry disabled, final no-op')
finally:
    server.shutdown()
    server.server_close()
