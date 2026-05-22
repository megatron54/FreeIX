import { useEffect, useState } from "react";
import { api, AppConfig, DnsProvider, ProtectionStatus } from "../lib/api";

export default function Settings() {
  const [config, setConfig] = useState<AppConfig | null>(null);
  const [providers, setProviders] = useState<DnsProvider[]>([]);
  const [status, setStatus] = useState<ProtectionStatus | null>(null);
  const [saving, setSaving] = useState(false);
  const [updating, setUpdating] = useState(false);
  const [updateMsg, setUpdateMsg] = useState("");
  const [autoUpdateInterval, setAutoUpdateInterval] = useState("24h");

  useEffect(() => {
    (async () => {
      const [c, p, s] = await Promise.all([
        api.getConfig(),
        api.getDnsProviders(),
        api.getStatus(),
      ]);
      setConfig(c);
      setProviders(p);
      setStatus(s);
    })();
  }, []);

  const handleSave = async () => {
    if (!config) return;
    setSaving(true);
    try {
      await api.updateConfig(config);
    } finally {
      setSaving(false);
    }
  };

  const handleProviderChange = async (id: string) => {
    await api.setDnsProvider(id);
    setConfig((prev) => (prev ? { ...prev, dns_provider_id: id } : null));
  };

  const handleUpdateBlocklists = async () => {
    setUpdating(true);
    setUpdateMsg("");
    try {
      const msg = await api.updateBlocklists();
      setUpdateMsg(msg);
    } catch (e) {
      setUpdateMsg(`Error: ${e}`);
    }
    setUpdating(false);
  };

  if (!config) {
    return (
      <div className="flex items-center justify-center h-full">
        <div className="animate-spin rounded-full h-8 w-8 border-b-2 border-blue-500" />
      </div>
    );
  }

  const protectionActive = status?.enabled ?? false;

  return (
    <div className="space-y-6 max-w-2xl">
      <h1 className="text-2xl font-bold text-gray-900 dark:text-white">
        Settings
      </h1>

      {/* DNS Provider */}
      <section className="bg-white dark:bg-gray-800 rounded-xl p-5 shadow-sm border border-gray-200 dark:border-gray-700 space-y-3">
        <h2 className="text-lg font-semibold text-gray-900 dark:text-white">
          DNS Provider
        </h2>
        <div className="space-y-2">
          {providers.map((p) => (
            <label
              key={p.id}
              className={`flex items-center gap-3 p-3 rounded-lg border cursor-pointer transition-colors ${
                config.dns_provider_id === p.id
                  ? "border-blue-500 bg-blue-50 dark:bg-blue-900/20"
                  : "border-gray-200 dark:border-gray-700 hover:border-gray-300"
              }`}
            >
              <input
                type="radio"
                name="dns-provider"
                value={p.id}
                checked={config.dns_provider_id === p.id}
                onChange={() => handleProviderChange(p.id)}
                className="text-blue-500"
              />
              <div>
                <p className="font-medium text-gray-900 dark:text-white">
                  {p.name}
                </p>
                <p className="text-xs text-gray-500 dark:text-gray-400">
                  {p.description}
                </p>
              </div>
            </label>
          ))}
        </div>
      </section>

      {/* General Settings */}
      <section className="bg-white dark:bg-gray-800 rounded-xl p-5 shadow-sm border border-gray-200 dark:border-gray-700 space-y-4">
        <h2 className="text-lg font-semibold text-gray-900 dark:text-white">
          General
        </h2>

        <div className="flex items-center justify-between">
          <div>
            <p className="font-medium text-gray-900 dark:text-white">
              Auto-start
            </p>
            <p className="text-xs text-gray-500 dark:text-gray-400">
              Enable DNS protection automatically on launch
            </p>
          </div>
          <button
            onClick={() =>
              setConfig({ ...config, auto_start: !config.auto_start })
            }
            className={`relative inline-flex h-6 w-11 items-center rounded-full transition-colors ${
              config.auto_start ? "bg-blue-500" : "bg-gray-300 dark:bg-gray-600"
            }`}
          >
            <span
              className={`inline-block h-4 w-4 transform rounded-full bg-white transition-transform ${
                config.auto_start ? "translate-x-6" : "translate-x-1"
              }`}
            />
          </button>
        </div>

        <div className="flex items-center justify-between">
          <div>
            <p className="font-medium text-gray-900 dark:text-white">
              Dark Mode
            </p>
            <p className="text-xs text-gray-500 dark:text-gray-400">
              Use dark theme
            </p>
          </div>
          <button
            onClick={() =>
              setConfig({ ...config, dark_mode: !config.dark_mode })
            }
            className={`relative inline-flex h-6 w-11 items-center rounded-full transition-colors ${
              config.dark_mode ? "bg-blue-500" : "bg-gray-300 dark:bg-gray-600"
            }`}
          >
            <span
              className={`inline-block h-4 w-4 transform rounded-full bg-white transition-transform ${
                config.dark_mode ? "translate-x-6" : "translate-x-1"
              }`}
            />
          </button>
        </div>
      </section>

      {/* Network Settings */}
      <section className="bg-white dark:bg-gray-800 rounded-xl p-5 shadow-sm border border-gray-200 dark:border-gray-700 space-y-4">
        <h2 className="text-lg font-semibold text-gray-900 dark:text-white">
          Network
        </h2>
        {protectionActive && (
          <p className="text-xs text-yellow-600 dark:text-yellow-400">
            Disable protection to change network settings.
          </p>
        )}

        <div>
          <label className="block font-medium text-gray-900 dark:text-white mb-1">
            Listen Address
          </label>
          <input
            type="text"
            value={config.listen_address}
            disabled={protectionActive}
            onChange={(e) =>
              setConfig({ ...config, listen_address: e.target.value })
            }
            className="w-full px-3 py-2 rounded-lg border border-gray-300 dark:border-gray-600 bg-white dark:bg-gray-700 text-gray-900 dark:text-white focus:ring-2 focus:ring-blue-500 outline-none disabled:opacity-50 disabled:cursor-not-allowed"
          />
        </div>

        <div>
          <label className="block font-medium text-gray-900 dark:text-white mb-1">
            Port
          </label>
          <input
            type="number"
            value={config.port}
            disabled={protectionActive}
            onChange={(e) =>
              setConfig({ ...config, port: parseInt(e.target.value) || 53 })
            }
            className="w-full px-3 py-2 rounded-lg border border-gray-300 dark:border-gray-600 bg-white dark:bg-gray-700 text-gray-900 dark:text-white focus:ring-2 focus:ring-blue-500 outline-none disabled:opacity-50 disabled:cursor-not-allowed"
          />
        </div>

        <div>
          <label className="block font-medium text-gray-900 dark:text-white mb-1">
            Cache Size
          </label>
          <input
            type="number"
            value={config.cache_size}
            onChange={(e) =>
              setConfig({ ...config, cache_size: parseInt(e.target.value) || 0 })
            }
            className="w-full px-3 py-2 rounded-lg border border-gray-300 dark:border-gray-600 bg-white dark:bg-gray-700 text-gray-900 dark:text-white focus:ring-2 focus:ring-blue-500 outline-none"
          />
          <p className="text-xs text-gray-500 dark:text-gray-400 mt-1">
            Number of DNS entries to cache
          </p>
        </div>
      </section>

      {/* Blocklist Management */}
      <section className="bg-white dark:bg-gray-800 rounded-xl p-5 shadow-sm border border-gray-200 dark:border-gray-700 space-y-4">
        <div className="flex items-center justify-between">
          <h2 className="text-lg font-semibold text-gray-900 dark:text-white">
            Blocklists
          </h2>
          <button
            onClick={handleUpdateBlocklists}
            disabled={updating}
            className="px-4 py-2 bg-blue-600 text-white rounded-lg text-sm hover:bg-blue-700 disabled:opacity-50"
          >
            {updating ? "Updating..." : "Update Now"}
          </button>
        </div>

        {updateMsg && (
          <div className="p-3 bg-green-50 dark:bg-green-900/20 border border-green-200 dark:border-green-800 rounded-lg text-sm text-green-700 dark:text-green-300">
            {updateMsg}
          </div>
        )}

        {status && (
          <p className="text-sm text-gray-600 dark:text-gray-300">
            {status.total_rules.toLocaleString()} blocking rules loaded
          </p>
        )}

        <div>
          <label className="block font-medium text-gray-900 dark:text-white mb-1">
            Auto-update Interval
          </label>
          <select
            value={autoUpdateInterval}
            onChange={(e) => setAutoUpdateInterval(e.target.value)}
            className="w-full px-3 py-2 rounded-lg border border-gray-300 dark:border-gray-600 bg-white dark:bg-gray-700 text-gray-900 dark:text-white focus:ring-2 focus:ring-blue-500 outline-none"
          >
            <option value="off">Off</option>
            <option value="12h">Every 12 hours</option>
            <option value="24h">Every 24 hours</option>
            <option value="48h">Every 48 hours</option>
          </select>
        </div>
      </section>

      <button
        onClick={handleSave}
        disabled={saving}
        className="w-full py-2.5 bg-blue-500 hover:bg-blue-600 text-white font-medium rounded-lg transition-colors disabled:opacity-50"
      >
        {saving ? "Saving..." : "Save Settings"}
      </button>
    </div>
  );
}
