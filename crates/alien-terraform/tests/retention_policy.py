"""Provider-state boundary cases for scoped retention preparation."""
import copy
import importlib.util
import unittest
from unittest.mock import patch

spec = importlib.util.spec_from_file_location('retention', 'retain-data.py')
retention = importlib.util.module_from_spec(spec)
spec.loader.exec_module(retention)


class AzureRetention(unittest.TestCase):
    def setUp(self):
        self.rule = {'enabled': True, 'filters': [{'prefix_match': ['data/']}],
                     'actions': [{'base_blob': [{'delete_after_days_since_modification_greater_than': 0}]}]}
        self.state = {'resources': [
            {'mode': 'managed', 'type': 'azurerm_storage_account', 'name': 'shared',
             'instances': [{'attributes': {'id': 'account'}}]},
            {'mode': 'managed', 'type': 'azurerm_storage_container', 'name': 'data',
             'instances': [{'attributes': {'id': 'container', 'name': 'data', 'storage_account_id': 'account'}}]},
            {'mode': 'managed', 'type': 'azurerm_storage_management_policy', 'name': 'shared',
             'instances': [{'attributes': {'storage_account_id': 'account', 'rule': [self.rule]}}]},
        ]}
        self.selected = {'azurerm_storage_container.data'}

    def test_noop_and_postgres_only_removal_preserve_active_storage_policy(self):
        retention.azure_expiry_guard(self.state, set())
        retention.azure_expiry_guard(self.state, {'azurerm_postgresql_flexible_server.database'})

    def test_data_source_with_same_name_is_not_detached(self):
        foreign = copy.deepcopy(self.state['resources'][1])
        foreign['mode'] = 'data'
        foreign['instances'][0]['attributes'] = {'id': 'foreign'}
        self.state['resources'].append(foreign)
        self.rule['enabled'] = False
        retention.azure_expiry_guard(self.state, self.selected)

    def test_container_removal_blocks_applicable_zero_day_expiry(self):
        with self.assertRaisesRegex(RuntimeError, 'Disable Azure storage lifecycle'):
            retention.azure_expiry_guard(self.state, self.selected)

    def test_name_based_container_state_preserves_expiry_guard(self):
        account = '/subscriptions/sub/resourceGroups/group/providers/Microsoft.Storage/storageAccounts/shared'
        attributes = self.state['resources'][1]['instances'][0]['attributes']
        attributes.update(storage_account_id=None, storage_account_name='shared',
                          resource_manager_id=account + '/blobServices/default/containers/data')
        self.state['resources'][2]['instances'][0]['attributes']['storage_account_id'] = account
        with self.assertRaisesRegex(RuntimeError, 'Disable Azure storage lifecycle'):
            retention.azure_expiry_guard(self.state, self.selected)
        self.rule['enabled'] = False
        retention.azure_expiry_guard(self.state, self.selected)

        for invalid in [None, 'https://shared.blob.core.windows.net/data',
                        account + '/blobServices/default/containers/other',
                        account.replace('/shared', '/other') + '/blobServices/default/containers/data']:
            with self.subTest(resource_manager_id=invalid):
                attributes['resource_manager_id'] = invalid
                with self.assertRaisesRegex(RuntimeError, 'Cannot identify retained Azure container'):
                    retention.azure_expiry_guard(self.state, self.selected)

    def test_unrelated_prefix_and_disabled_delete_do_not_block(self):
        for prefix in ['sibling/', 'database/']:
            self.rule['filters'][0]['prefix_match'] = [prefix]
            retention.azure_expiry_guard(self.state, self.selected)
        self.rule['filters'] = []
        self.rule['enabled'] = False
        retention.azure_expiry_guard(self.state, self.selected)

    def test_whole_account_detach_includes_all_containers(self):
        self.rule['filters'][0]['prefix_match'] = ['sibling/']
        with self.assertRaisesRegex(RuntimeError, 'Disable Azure storage lifecycle'):
            retention.azure_expiry_guard(self.state, {'azurerm_storage_account.shared'})

    def test_tiering_and_unset_delete_ages_are_preserved(self):
        self.rule['actions'] = [{'base_blob': [{
            'tier_to_cool_after_days_since_modification_greater_than': 1,
            'delete_after_days_since_modification_greater_than': -1,
            'delete_after_days_since_creation_greater_than': None}]}]
        retention.azure_expiry_guard(self.state, self.selected)

    def test_snapshot_version_broad_prefix_and_unknown_actions_fail_closed(self):
        for kind in ['snapshot', 'version']:
            for prefix in ['', 'da', 'data/', 'data/nested/']:
                rule = copy.deepcopy(self.rule)
                rule['filters'][0]['prefix_match'] = [prefix]
                rule['actions'] = [{kind: [{'delete_after_days_since_creation_greater_than': 2}]}]
                self.state['resources'][2]['instances'][0]['attributes']['rule'] = [rule]
                with self.assertRaisesRegex(RuntimeError, 'Disable Azure storage lifecycle'):
                    retention.azure_expiry_guard(self.state, self.selected)
        rule['actions'] = [{'future_action': [{}]}]
        with self.assertRaisesRegex(RuntimeError, 'Unknown applicable'):
            retention.azure_expiry_guard(self.state, self.selected)


class DynamoRetention(unittest.TestCase):
    def setUp(self):
        self.arn = 'arn:aws:dynamodb:us-east-2:123456789012:table/owned-data'
        self.state = {'resources': [{'mode': 'managed', 'type': 'aws_dynamodb_table',
            'name': 'data', 'instances': [{'attributes': {'id': 'owned-data', 'arn': self.arn}}]}]}
        self.selected = [{'address': 'aws_dynamodb_table.data', 'id': 'owned-data'}]

    def invoke(self, observations):
        calls = []
        def aws(arguments, **kwargs):
            calls.append(arguments)
            self.assertEqual(arguments[arguments.index('--table-name') + 1], self.arn)
            return retention.json.dumps(observations.pop(0))
        with patch.object(retention.subprocess, 'check_output', side_effect=aws):
            retention.protect_dynamodb_ttl(self.state, self.selected)
        return calls

    def test_disable_uses_observed_custom_attribute_and_confirms_before_detach(self):
        calls = self.invoke([{'Table': {'TableArn': self.arn}},
            {'TimeToLiveDescription': {'TimeToLiveStatus': 'ENABLED', 'AttributeName': 'custom_expiry'}},
            {}, {'TimeToLiveDescription': {'TimeToLiveStatus': 'DISABLED'}}])
        self.assertEqual([call[2] for call in calls],
            ['describe-table', 'describe-time-to-live', 'update-time-to-live', 'describe-time-to-live'])
        request = retention.json.loads(calls[2][calls[2].index('--time-to-live-specification') + 1])
        self.assertEqual(request, {'Enabled': False, 'AttributeName': 'custom_expiry'})

    def test_transition_unknown_identity_and_denied_reads_never_authorize_detach(self):
        for status in ['ENABLING', 'DISABLING', 'UNKNOWN', None]:
            with self.assertRaisesRegex(RuntimeError, 'not confirmed DISABLED'):
                self.invoke([{'Table': {'TableArn': self.arn}},
                    {'TimeToLiveDescription': {'TimeToLiveStatus': status}}])
        with self.assertRaisesRegex(RuntimeError, 'identity changed'):
            self.invoke([{'Table': {'TableArn': self.arn.replace('123456789012', '999999999999')}}])
        with patch.object(retention.subprocess, 'check_output', side_effect=retention.subprocess.CalledProcessError(1, 'aws')):
            with self.assertRaisesRegex(RuntimeError, 'installer credentials'):
                retention.protect_dynamodb_ttl(self.state, self.selected)

    def test_noop_and_declaration_removal_touch_only_selected_tables(self):
        with patch.object(retention.subprocess, 'check_output') as aws:
            retention.protect_dynamodb_ttl(self.state, [])
            aws.assert_not_called()
        calls = self.invoke([{'Table': {'TableArn': self.arn}},
            {'TimeToLiveDescription': {'TimeToLiveStatus': 'DISABLED'}}])
        self.assertEqual(len(calls), 2)


if __name__ == '__main__':
    unittest.main()
