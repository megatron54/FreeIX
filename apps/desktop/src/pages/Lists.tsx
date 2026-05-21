import { useState } from "react";
import { api } from "../lib/api";

type Tab = "whitelist" | "blacklist";

export default function Lists() {
  const [tab, setTab] = useState<Tab>("whitelist");
  const [whitelist, setWhitelist] = useState<string[]>([]);
  const [blacklist, setBlacklist] = useState<string[]>([]);
  const [input, setInput] = useState("");

  const currentList = tab === "whitelist" ? whitelist : blacklist;

  const handleAdd = async () => {
    const domain = input.trim().toLowerCase();
    if (!domain) return;
    if (tab === "whitelist") {
      await api.addWhitelist(domain);
      setWhitelist((prev) => [...prev, domain]);
    } else {
      await api.addBlacklist(domain);
      setBlacklist((prev) => [...prev, domain]);
    }
    setInput("");
  };

  const handleRemove = async (domain: string) => {
    if (tab === "whitelist") {
      await api.removeWhitelist(domain);
      setWhitelist((prev) => prev.filter((d) => d !== domain));
    } else {
      await api.removeBlacklist(domain);
      setBlacklist((prev) => prev.filter((d) => d !== domain));
    }
  };

  const handleImport = () => {
    const fileInput = document.createElement("input");
    fileInput.type = "file";
    fileInput.accept = ".txt";
    fileInput.onchange = async (e) => {
      const file = (e.target as HTMLInputElement).files?.[0];
      if (!file) return;
      const text = await file.text();
      const domains = text
        .split("\n")
        .map((l) => l.trim().toLowerCase())
        .filter((l) => l && !l.startsWith("#"));
      for (const domain of domains) {
        if (tab === "whitelist") {
          await api.addWhitelist(domain);
        } else {
          await api.addBlacklist(domain);
        }
      }
      if (tab === "whitelist") {
        setWhitelist((prev) => [...new Set([...prev, ...domains])]);
      } else {
        setBlacklist((prev) => [...new Set([...prev, ...domains])]);
      }
    };
    fileInput.click();
  };

  return (
    <div className="p-6 space-y-6 max-w-2xl">
      <h1 className="text-2xl font-bold text-gray-900 dark:text-white">
        Domain Lists
      </h1>

      {/* Tabs */}
      <div className="flex gap-1 bg-gray-100 dark:bg-gray-800 rounded-lg p-1">
        <button
          onClick={() => setTab("whitelist")}
          className={`flex-1 py-2 px-4 rounded-md text-sm font-medium transition-colors ${
            tab === "whitelist"
              ? "bg-white dark:bg-gray-700 text-gray-900 dark:text-white shadow-sm"
              : "text-gray-600 dark:text-gray-400"
          }`}
        >
          Whitelist ({whitelist.length})
        </button>
        <button
          onClick={() => setTab("blacklist")}
          className={`flex-1 py-2 px-4 rounded-md text-sm font-medium transition-colors ${
            tab === "blacklist"
              ? "bg-white dark:bg-gray-700 text-gray-900 dark:text-white shadow-sm"
              : "text-gray-600 dark:text-gray-400"
          }`}
        >
          Blacklist ({blacklist.length})
        </button>
      </div>

      {/* Add Domain */}
      <div className="flex gap-2">
        <input
          type="text"
          placeholder="Enter domain (e.g. example.com)"
          value={input}
          onChange={(e) => setInput(e.target.value)}
          onKeyDown={(e) => e.key === "Enter" && handleAdd()}
          className="flex-1 px-3 py-2 rounded-lg border border-gray-300 dark:border-gray-600 bg-white dark:bg-gray-700 text-gray-900 dark:text-white focus:ring-2 focus:ring-blue-500 outline-none"
        />
        <button
          onClick={handleAdd}
          className="px-4 py-2 bg-blue-500 hover:bg-blue-600 text-white font-medium rounded-lg transition-colors"
        >
          Add
        </button>
        <button
          onClick={handleImport}
          className="px-4 py-2 bg-gray-200 dark:bg-gray-700 hover:bg-gray-300 dark:hover:bg-gray-600 text-gray-700 dark:text-gray-300 font-medium rounded-lg transition-colors"
        >
          Import
        </button>
      </div>

      {/* Domain List */}
      <div className="bg-white dark:bg-gray-800 rounded-xl border border-gray-200 dark:border-gray-700 divide-y divide-gray-200 dark:divide-gray-700 max-h-96 overflow-y-auto">
        {currentList.length === 0 ? (
          <div className="p-8 text-center text-gray-500 dark:text-gray-400">
            No domains in {tab}. Add one above.
          </div>
        ) : (
          currentList.map((domain) => (
            <div
              key={domain}
              className="flex items-center justify-between px-4 py-3"
            >
              <span className="text-sm text-gray-900 dark:text-white font-mono">
                {domain}
              </span>
              <button
                onClick={() => handleRemove(domain)}
                className="text-red-500 hover:text-red-600 text-sm font-medium"
              >
                Remove
              </button>
            </div>
          ))
        )}
      </div>
    </div>
  );
}
