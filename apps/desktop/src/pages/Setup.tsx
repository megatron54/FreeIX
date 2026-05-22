import { useState } from "react";
import { invoke } from "@tauri-apps/api/core";

interface SetupProps {
  onComplete: () => void;
}

export default function Setup({ onComplete }: SetupProps) {
  const [step, setStep] = useState(0);
  const [autoStart, setAutoStart] = useState(true);
  const [dnsSet, setDnsSet] = useState(false);
  const [settingDns, setSettingDns] = useState(false);
  const [caInstalled, setCaInstalled] = useState(false);
  const [installingCa, setInstallingCa] = useState(false);

  const steps = [
    // Step 0: Welcome
    <div className="text-center space-y-6">
      <div className="w-20 h-20 mx-auto bg-teal-500 rounded-full flex items-center justify-center">
        <span className="text-white text-3xl font-bold">F</span>
      </div>
      <h1 className="text-3xl font-bold text-white">Welcome to FreeIX</h1>
      <p className="text-zinc-400 max-w-md mx-auto">
        FreeIX blocks all ads, trackers, and malware across your entire system —
        including YouTube video ads, banner ads, and tracking scripts.
      </p>
      <p className="text-zinc-500 text-sm max-w-md mx-auto">
        We need to configure a few permissions. This only takes a moment.
      </p>
    </div>,

    // Step 1: DNS Permission
    <div className="space-y-6">
      <h2 className="text-2xl font-bold text-white">Set System DNS</h2>
      <p className="text-zinc-400">
        FreeIX intercepts DNS queries to block ad domains at the network level.
        This requires administrator permission to change your DNS settings.
      </p>
      <div className="bg-zinc-800 rounded-lg p-4 space-y-2">
        <p className="text-zinc-300 text-sm font-medium">What this does:</p>
        <ul className="text-zinc-400 text-sm space-y-1 list-disc list-inside">
          <li>Redirects DNS queries through FreeIX (127.0.0.1)</li>
          <li>Blocks thousands of known ad and tracker domains</li>
          <li>Automatically restored when FreeIX exits</li>
        </ul>
      </div>
      <button
        onClick={async () => {
          setSettingDns(true);
          try {
            await invoke("set_system_dns_to_local");
            setDnsSet(true);
          } catch (e) {
            console.error("DNS set failed:", e);
          }
          setSettingDns(false);
        }}
        disabled={settingDns || dnsSet}
        className={`w-full py-3 rounded-lg font-medium transition ${
          dnsSet
            ? "bg-green-600 text-white cursor-default"
            : settingDns
            ? "bg-zinc-700 text-zinc-400 cursor-wait"
            : "bg-teal-500 hover:bg-teal-600 text-white"
        }`}
      >
        {dnsSet ? "DNS Configured" : settingDns ? "Waiting for permission..." : "Grant Permission"}
      </button>
    </div>,

    // Step 2: Install CA Certificate (for HTTPS ad blocking)
    <div className="space-y-6">
      <h2 className="text-2xl font-bold text-white">Install Security Certificate</h2>
      <p className="text-zinc-400">
        To block ads inside encrypted connections (like YouTube video ads),
        FreeIX needs to install a local security certificate on your system.
      </p>
      <div className="bg-zinc-800 rounded-lg p-4 space-y-2">
        <p className="text-zinc-300 text-sm font-medium">What this does:</p>
        <ul className="text-zinc-400 text-sm space-y-1 list-disc list-inside">
          <li>Installs a local-only FreeIX root certificate</li>
          <li>Allows FreeIX to inspect HTTPS ad requests</li>
          <li>Blocks YouTube ads, tracking scripts, and more</li>
          <li>Your data never leaves your computer</li>
        </ul>
      </div>
      <div className="bg-amber-900/30 border border-amber-700 rounded-lg p-3">
        <p className="text-amber-300 text-sm">
          This certificate is generated locally and unique to your device. 
          It is never shared with anyone and can be removed at any time.
        </p>
      </div>
      <button
        onClick={async () => {
          setInstallingCa(true);
          try {
            await invoke("install_ca_certificate");
            setCaInstalled(true);
          } catch (e) {
            console.error("CA install failed:", e);
          }
          setInstallingCa(false);
        }}
        disabled={installingCa || caInstalled}
        className={`w-full py-3 rounded-lg font-medium transition ${
          caInstalled
            ? "bg-green-600 text-white cursor-default"
            : installingCa
            ? "bg-zinc-700 text-zinc-400 cursor-wait"
            : "bg-teal-500 hover:bg-teal-600 text-white"
        }`}
      >
        {caInstalled ? "Certificate Installed" : installingCa ? "Installing..." : "Install Certificate"}
      </button>
    </div>,

    // Step 3: Auto-start
    <div className="space-y-6">
      <h2 className="text-2xl font-bold text-white">Start on Boot</h2>
      <p className="text-zinc-400">
        Would you like FreeIX to start automatically when you log in?
        This ensures protection is always active.
      </p>
      <label className="flex items-center justify-between bg-zinc-800 rounded-lg p-4 cursor-pointer">
        <span className="text-white">Start FreeIX on boot</span>
        <input
          type="checkbox"
          checked={autoStart}
          onChange={(e) => setAutoStart(e.target.checked)}
          className="w-5 h-5 accent-teal-500"
        />
      </label>
    </div>,

    // Step 4: Done
    <div className="text-center space-y-6">
      <div className="w-20 h-20 mx-auto bg-green-500 rounded-full flex items-center justify-center">
        <svg className="w-10 h-10 text-white" fill="none" viewBox="0 0 24 24" stroke="currentColor">
          <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={3} d="M5 13l4 4L19 7" />
        </svg>
      </div>
      <h2 className="text-2xl font-bold text-white">You're All Set!</h2>
      <p className="text-zinc-400 max-w-md mx-auto">
        FreeIX is now blocking ads, trackers, and malware across your entire system —
        including YouTube video ads.
      </p>
      {!caInstalled && (
        <p className="text-amber-400 text-sm">
          Note: You skipped the certificate installation. YouTube ads may still appear.
          You can install it later in Settings.
        </p>
      )}
    </div>,
  ];

  const isLast = step === steps.length - 1;

  return (
    <div className="min-h-screen bg-zinc-900 flex items-center justify-center p-8">
      <div className="w-full max-w-lg space-y-8">
        {/* Progress dots */}
        <div className="flex justify-center gap-2">
          {steps.map((_, i) => (
            <div
              key={i}
              className={`w-2.5 h-2.5 rounded-full transition ${
                i === step ? "bg-teal-500" : i < step ? "bg-teal-800" : "bg-zinc-700"
              }`}
            />
          ))}
        </div>

        {/* Content */}
        <div className="min-h-[320px] flex items-center">
          <div className="w-full">{steps[step]}</div>
        </div>

        {/* Navigation */}
        <div className="flex justify-between">
          {step > 0 ? (
            <button
              onClick={() => setStep(step - 1)}
              className="px-6 py-2 text-zinc-400 hover:text-white transition"
            >
              Back
            </button>
          ) : (
            <div />
          )}
          <button
            onClick={async () => {
              if (isLast) {
                try {
                  await invoke("update_config", {
                    config: { auto_start: autoStart },
                  });
                  await invoke("complete_setup");
                } catch (e) {
                  console.error(e);
                }
                onComplete();
              } else {
                setStep(step + 1);
              }
            }}
            className="px-6 py-2 bg-teal-500 hover:bg-teal-600 text-white rounded-lg font-medium transition"
          >
            {isLast ? "Get Started" : step === 0 ? "Let's Go" : "Next"}
          </button>
        </div>
      </div>
    </div>
  );
}
