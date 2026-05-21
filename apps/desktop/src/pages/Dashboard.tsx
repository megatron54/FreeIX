import { useState, useEffect } from "react";
import { api, ProtectionStatus, StatsResponse, QueryEvent } from "../lib/api";

export default function Dashboard() {
  const [status, setStatus] = useState<ProtectionStatus | null>(null);
  const [stats, setStats] = useState<StatsResponse | null>(null);
  const [logs, setLogs] = useState<QueryEvent[]>([]);
  const [toggling, setToggling] = useState(false);

  useEffect(() => {
    const poll = async () => {
      try {
        const [s, st, l] = await Promise.all([
          api.getStatus(),
          api.getStats(),
          api.getLogs(50),
        ]);
        setStatus(s);
        setStats(st);
        setLogs(l);
      } catch (e) {
        console.error("poll error", e);
      }
    };
    poll();
    const interval = setInterval(poll, 1000);
    return () => clearInterval(interval);
  }, []);

  const handleToggle = async () => {
    if (!status) return;
    setToggling(true);
    try {
      await api.toggleProtection(!status.enabled);
      const s = await api.getStatus();
      setStatus(s);
    } catch (e) {
      console.error("toggle error", e);
    }
    setToggling(false);
  };

  const formatUptime = (seconds: number) => {
    const h = Math.floor(seconds / 3600);
    const m = Math.floor((seconds % 3600) / 60);
    const s = seconds % 60;
    if (h > 0) return `${h}h ${m}m`;
    if (m > 0) return `${m}m ${s}s`;
    return `${s}s`;
  };

  return (
    <div className="space-y-6">
      {/* Toggle & Status */}
      <div className="flex items-center justify-between">
        <div>
          <h1 className="text-2xl font-bold text-gray-900 dark:text-white">
            FreeIX
          </h1>
          <p className="text-sm text-gray-500 dark:text-gray-400">
            {status?.enabled ? "Protection Active" : "Protection Disabled"}
          </p>
        </div>
        <button
          onClick={handleToggle}
          disabled={toggling}
          className={`relative w-20 h-20 rounded-full border-4 transition-all duration-300 flex items-center justify-center ${
            status?.enabled
              ? "border-green-500 bg-green-500/10 shadow-lg shadow-green-500/20"
              : "border-gray-400 bg-gray-100 dark:bg-gray-800"
          } ${toggling ? "opacity-50" : "hover:scale-105"}`}
        >
          <div
            className={`w-4 h-4 rounded-full ${
              status?.enabled ? "bg-green-500 animate-pulse" : "bg-gray-400"
            }`}
          />
        </button>
      </div>

      {/* Stats Cards */}
      <div className="grid grid-cols-2 lg:grid-cols-4 gap-4">
        <StatCard
          label="Total Queries"
          value={stats?.total_queries?.toLocaleString() ?? "0"}
          color="blue"
        />
        <StatCard
          label="Blocked"
          value={stats?.blocked_queries?.toLocaleString() ?? "0"}
          sub={stats ? `${stats.block_percentage.toFixed(1)}%` : ""}
          color="red"
        />
        <StatCard
          label="Cache Hits"
          value={stats?.cache_hits?.toLocaleString() ?? "0"}
          color="green"
        />
        <StatCard
          label="Uptime"
          value={stats ? formatUptime(stats.uptime_seconds) : "-"}
          color="purple"
        />
      </div>

      {/* Rules info */}
      {status && (
        <div className="text-sm text-gray-500 dark:text-gray-400">
          {status.total_rules.toLocaleString()} blocking rules loaded · Provider: {status.dns_provider}
        </div>
      )}

      {/* Live Query Log */}
      <div>
        <h2 className="text-lg font-semibold text-gray-900 dark:text-white mb-3">
          Live DNS Queries
        </h2>
        <div className="bg-white dark:bg-gray-800 rounded-lg border border-gray-200 dark:border-gray-700 overflow-hidden">
          <div className="max-h-80 overflow-y-auto">
            {logs.length === 0 ? (
              <p className="p-4 text-gray-500 text-sm text-center">
                {status?.enabled
                  ? "Waiting for DNS queries..."
                  : "Enable protection to see queries"}
              </p>
            ) : (
              <table className="w-full text-sm">
                <thead className="bg-gray-50 dark:bg-gray-700 sticky top-0">
                  <tr>
                    <th className="px-3 py-2 text-left text-gray-500 dark:text-gray-400 font-medium">Domain</th>
                    <th className="px-3 py-2 text-left text-gray-500 dark:text-gray-400 font-medium">Status</th>
                    <th className="px-3 py-2 text-right text-gray-500 dark:text-gray-400 font-medium">Time</th>
                  </tr>
                </thead>
                <tbody className="divide-y divide-gray-100 dark:divide-gray-700">
                  {logs.map((log, i) => (
                    <tr
                      key={i}
                      className={`${
                        log.status === "blocked"
                          ? "bg-red-50 dark:bg-red-900/20"
                          : ""
                      }`}
                    >
                      <td className="px-3 py-1.5 font-mono text-xs truncate max-w-[250px]">
                        <span className={log.status === "blocked" ? "text-red-600 dark:text-red-400 font-semibold" : "text-gray-700 dark:text-gray-300"}>
                          {log.domain}
                        </span>
                      </td>
                      <td className="px-3 py-1.5">
                        <StatusBadge status={log.status} />
                      </td>
                      <td className="px-3 py-1.5 text-right text-gray-400 text-xs">
                        {log.response_time_ms}ms
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
            )}
          </div>
        </div>
      </div>
    </div>
  );
}

function StatCard({ label, value, sub, color }: { label: string; value: string; sub?: string; color: string }) {
  const colors: Record<string, string> = {
    blue: "border-blue-500/30 bg-blue-50 dark:bg-blue-900/20",
    red: "border-red-500/30 bg-red-50 dark:bg-red-900/20",
    green: "border-green-500/30 bg-green-50 dark:bg-green-900/20",
    purple: "border-purple-500/30 bg-purple-50 dark:bg-purple-900/20",
  };
  return (
    <div className={`rounded-lg border p-4 ${colors[color]}`}>
      <p className="text-xs text-gray-500 dark:text-gray-400 uppercase tracking-wide">{label}</p>
      <p className="text-2xl font-bold text-gray-900 dark:text-white mt-1">{value}</p>
      {sub && <p className="text-xs text-gray-500 mt-0.5">{sub}</p>}
    </div>
  );
}

function StatusBadge({ status }: { status: string }) {
  const styles: Record<string, string> = {
    allowed: "bg-green-100 text-green-700 dark:bg-green-800/40 dark:text-green-300",
    blocked: "bg-red-100 text-red-700 dark:bg-red-800/40 dark:text-red-300",
    cached: "bg-blue-100 text-blue-700 dark:bg-blue-800/40 dark:text-blue-300",
    error: "bg-yellow-100 text-yellow-700 dark:bg-yellow-800/40 dark:text-yellow-300",
  };
  return (
    <span className={`px-2 py-0.5 rounded text-xs font-medium ${styles[status] || styles.allowed}`}>
      {status}
    </span>
  );
}
