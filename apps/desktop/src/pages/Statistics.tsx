import { useState, useEffect } from "react";
import { api, StatsResponse, TopBlocked } from "../lib/api";

export default function Statistics() {
  const [stats, setStats] = useState<StatsResponse | null>(null);
  const [topBlocked, setTopBlocked] = useState<TopBlocked[]>([]);
  const [updating, setUpdating] = useState(false);
  const [updateMsg, setUpdateMsg] = useState("");

  useEffect(() => {
    const poll = async () => {
      try {
        const [s, t] = await Promise.all([api.getStats(), api.getTopBlocked()]);
        setStats(s);
        setTopBlocked(t);
      } catch (e) {
        console.error(e);
      }
    };
    poll();
    const interval = setInterval(poll, 2000);
    return () => clearInterval(interval);
  }, []);

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

  return (
    <div className="space-y-6">
      <div className="flex items-center justify-between">
        <h1 className="text-2xl font-bold text-gray-900 dark:text-white">
          Statistics
        </h1>
        <button
          onClick={handleUpdateBlocklists}
          disabled={updating}
          className="px-4 py-2 bg-blue-600 text-white rounded-lg text-sm hover:bg-blue-700 disabled:opacity-50"
        >
          {updating ? "Updating..." : "Update Blocklists"}
        </button>
      </div>

      {updateMsg && (
        <div className="p-3 bg-green-50 dark:bg-green-900/20 border border-green-200 dark:border-green-800 rounded-lg text-sm text-green-700 dark:text-green-300">
          {updateMsg}
        </div>
      )}

      {/* Summary */}
      <div className="grid grid-cols-2 lg:grid-cols-4 gap-4">
        <BigStat label="Total Queries" value={stats?.total_queries ?? 0} />
        <BigStat label="Blocked" value={stats?.blocked_queries ?? 0} color="red" />
        <BigStat label="Block Rate" value={`${(stats?.block_percentage ?? 0).toFixed(1)}%`} color="red" />
        <BigStat label="Cache Hits" value={stats?.cache_hits ?? 0} color="green" />
      </div>

      {/* Top Blocked Domains */}
      <div>
        <h2 className="text-lg font-semibold text-gray-900 dark:text-white mb-3">
          Top Blocked Domains
        </h2>
        {topBlocked.length === 0 ? (
          <p className="text-gray-500 text-sm">
            No blocked domains yet. Enable protection and browse the web.
          </p>
        ) : (
          <div className="bg-white dark:bg-gray-800 rounded-lg border border-gray-200 dark:border-gray-700 overflow-hidden">
            <table className="w-full text-sm">
              <thead className="bg-gray-50 dark:bg-gray-700">
                <tr>
                  <th className="px-4 py-3 text-left text-gray-500 dark:text-gray-400 font-medium">#</th>
                  <th className="px-4 py-3 text-left text-gray-500 dark:text-gray-400 font-medium">Domain</th>
                  <th className="px-4 py-3 text-right text-gray-500 dark:text-gray-400 font-medium">Blocked</th>
                  <th className="px-4 py-3 text-right text-gray-500 dark:text-gray-400 font-medium">% of Total</th>
                </tr>
              </thead>
              <tbody className="divide-y divide-gray-100 dark:divide-gray-700">
                {topBlocked.map((item, i) => (
                  <tr key={item.domain} className="hover:bg-gray-50 dark:hover:bg-gray-700/50">
                    <td className="px-4 py-2 text-gray-400">{i + 1}</td>
                    <td className="px-4 py-2 font-mono text-xs text-red-600 dark:text-red-400 font-semibold">
                      {item.domain}
                    </td>
                    <td className="px-4 py-2 text-right font-semibold text-gray-900 dark:text-white">
                      {item.count.toLocaleString()}
                    </td>
                    <td className="px-4 py-2 text-right text-gray-500">
                      {stats && stats.blocked_queries > 0
                        ? `${((item.count / stats.blocked_queries) * 100).toFixed(1)}%`
                        : "-"}
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        )}
      </div>

      {/* Categories */}
      <div>
        <h2 className="text-lg font-semibold text-gray-900 dark:text-white mb-3">
          What's Being Blocked
        </h2>
        <div className="grid grid-cols-1 md:grid-cols-3 gap-3">
          <CategoryCard
            title="Ads"
            description="Ad networks, banners, pop-ups"
            icon="🚫"
            examples={["doubleclick.net", "googlesyndication.com", "adnxs.com"]}
          />
          <CategoryCard
            title="Trackers"
            description="Analytics, fingerprinting, telemetry"
            icon="👁"
            examples={["google-analytics.com", "facebook.com/tr", "hotjar.com"]}
          />
          <CategoryCard
            title="Malware"
            description="Phishing, malware C&C, scams"
            icon="☠"
            examples={["malware-domain.com", "phishing-site.net"]}
          />
        </div>
      </div>
    </div>
  );
}

function BigStat({ label, value, color }: { label: string; value: number | string; color?: string }) {
  const textColor = color === "red" ? "text-red-600 dark:text-red-400" : color === "green" ? "text-green-600 dark:text-green-400" : "text-gray-900 dark:text-white";
  return (
    <div className="bg-white dark:bg-gray-800 rounded-lg border border-gray-200 dark:border-gray-700 p-4">
      <p className="text-xs text-gray-500 dark:text-gray-400 uppercase tracking-wide">{label}</p>
      <p className={`text-3xl font-bold mt-1 ${textColor}`}>
        {typeof value === "number" ? value.toLocaleString() : value}
      </p>
    </div>
  );
}

function CategoryCard({ title, description, icon, examples }: { title: string; description: string; icon: string; examples: string[] }) {
  return (
    <div className="bg-white dark:bg-gray-800 rounded-lg border border-gray-200 dark:border-gray-700 p-4">
      <div className="flex items-center gap-2 mb-2">
        <span className="text-lg">{icon}</span>
        <h3 className="font-semibold text-gray-900 dark:text-white">{title}</h3>
      </div>
      <p className="text-xs text-gray-500 dark:text-gray-400 mb-2">{description}</p>
      <div className="space-y-1">
        {examples.map((ex) => (
          <p key={ex} className="text-xs font-mono text-gray-400">{ex}</p>
        ))}
      </div>
    </div>
  );
}
