import { useEffect, useState } from "react";
import { api } from "../lib/api";

interface QueryLog {
  timestamp: number;
  domain: string;
  status: string;
  upstream: string;
  response_time_ms: number;
}

export default function Logs() {
  const [logs, setLogs] = useState<QueryLog[]>([]);
  const [filter, setFilter] = useState<"all" | "blocked">("all");
  const [search, setSearch] = useState("");

  useEffect(() => {
    const fetch = async () => {
      const data = await api.getLogs(500);
      setLogs(data);
    };
    fetch();
    const interval = setInterval(fetch, 3000);
    return () => clearInterval(interval);
  }, []);

  const filtered = logs.filter((log) => {
    if (filter === "blocked" && log.status !== "blocked") return false;
    if (search && !log.domain.includes(search.toLowerCase())) return false;
    return true;
  });

  return (
    <div className="p-6 space-y-4 h-full flex flex-col">
      <h1 className="text-2xl font-bold text-gray-900 dark:text-white">
        Query Logs
      </h1>

      {/* Filters */}
      <div className="flex gap-3 items-center">
        <input
          type="text"
          placeholder="Search domains..."
          value={search}
          onChange={(e) => setSearch(e.target.value)}
          className="flex-1 px-3 py-2 rounded-lg border border-gray-300 dark:border-gray-600 bg-white dark:bg-gray-700 text-gray-900 dark:text-white focus:ring-2 focus:ring-blue-500 outline-none text-sm"
        />
        <select
          value={filter}
          onChange={(e) => setFilter(e.target.value as "all" | "blocked")}
          className="px-3 py-2 rounded-lg border border-gray-300 dark:border-gray-600 bg-white dark:bg-gray-700 text-gray-900 dark:text-white text-sm outline-none"
        >
          <option value="all">All Queries</option>
          <option value="blocked">Blocked Only</option>
        </select>
      </div>

      {/* Table */}
      <div className="flex-1 overflow-auto bg-white dark:bg-gray-800 rounded-xl border border-gray-200 dark:border-gray-700">
        <table className="w-full text-sm">
          <thead className="sticky top-0 bg-gray-50 dark:bg-gray-900 border-b border-gray-200 dark:border-gray-700">
            <tr>
              <th className="text-left px-4 py-3 font-medium text-gray-500 dark:text-gray-400">
                Time
              </th>
              <th className="text-left px-4 py-3 font-medium text-gray-500 dark:text-gray-400">
                Domain
              </th>
              <th className="text-left px-4 py-3 font-medium text-gray-500 dark:text-gray-400">
                Status
              </th>
              <th className="text-left px-4 py-3 font-medium text-gray-500 dark:text-gray-400">
                Upstream
              </th>
              <th className="text-right px-4 py-3 font-medium text-gray-500 dark:text-gray-400">
                Time (ms)
              </th>
            </tr>
          </thead>
          <tbody className="divide-y divide-gray-100 dark:divide-gray-700">
            {filtered.length === 0 ? (
              <tr>
                <td
                  colSpan={5}
                  className="px-4 py-8 text-center text-gray-500 dark:text-gray-400"
                >
                  No logs to display
                </td>
              </tr>
            ) : (
              filtered.map((log, i) => (
                <tr
                  key={i}
                  className="hover:bg-gray-50 dark:hover:bg-gray-750"
                >
                  <td className="px-4 py-2 text-gray-600 dark:text-gray-400 font-mono text-xs">
                    {new Date(log.timestamp * 1000).toLocaleTimeString()}
                  </td>
                  <td className="px-4 py-2 text-gray-900 dark:text-white font-mono text-xs">
                    {log.domain}
                  </td>
                  <td className="px-4 py-2">
                    <span
                      className={`inline-flex px-2 py-0.5 rounded-full text-xs font-medium ${
                        log.status === "blocked"
                          ? "bg-red-100 dark:bg-red-900/30 text-red-700 dark:text-red-400"
                          : "bg-green-100 dark:bg-green-900/30 text-green-700 dark:text-green-400"
                      }`}
                    >
                      {log.status}
                    </span>
                  </td>
                  <td className="px-4 py-2 text-gray-600 dark:text-gray-400 text-xs">
                    {log.upstream}
                  </td>
                  <td className="px-4 py-2 text-right text-gray-600 dark:text-gray-400 text-xs">
                    {log.response_time_ms}
                  </td>
                </tr>
              ))
            )}
          </tbody>
        </table>
      </div>
    </div>
  );
}
