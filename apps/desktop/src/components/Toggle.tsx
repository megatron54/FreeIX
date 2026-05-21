interface ToggleProps {
  enabled: boolean;
  onToggle: (enabled: boolean) => void;
}

export default function Toggle({ enabled, onToggle }: ToggleProps) {
  return (
    <button
      onClick={() => onToggle(!enabled)}
      className={`relative w-32 h-32 rounded-full transition-all duration-500 shadow-lg focus:outline-none focus:ring-4 focus:ring-offset-2 ${
        enabled
          ? "bg-gradient-to-br from-green-400 to-green-600 focus:ring-green-300 shadow-green-500/30"
          : "bg-gradient-to-br from-red-400 to-red-600 focus:ring-red-300 shadow-red-500/30"
      }`}
    >
      {/* Inner circle */}
      <div
        className={`absolute inset-3 rounded-full bg-white/20 backdrop-blur-sm flex items-center justify-center transition-all duration-500 ${
          enabled ? "scale-100" : "scale-95"
        }`}
      >
        <div
          className={`w-12 h-12 rounded-full transition-all duration-500 ${
            enabled ? "bg-white shadow-lg" : "bg-white/60"
          }`}
        >
          <div className="w-full h-full flex items-center justify-center">
            {enabled ? (
              <svg
                className="w-6 h-6 text-green-600"
                fill="none"
                viewBox="0 0 24 24"
                stroke="currentColor"
                strokeWidth={3}
              >
                <path
                  strokeLinecap="round"
                  strokeLinejoin="round"
                  d="M5 13l4 4L19 7"
                />
              </svg>
            ) : (
              <svg
                className="w-6 h-6 text-red-600"
                fill="none"
                viewBox="0 0 24 24"
                stroke="currentColor"
                strokeWidth={3}
              >
                <path
                  strokeLinecap="round"
                  strokeLinejoin="round"
                  d="M6 18L18 6M6 6l12 12"
                />
              </svg>
            )}
          </div>
        </div>
      </div>

      {/* Pulse animation when enabled */}
      {enabled && (
        <div className="absolute inset-0 rounded-full bg-green-400 animate-ping opacity-20" />
      )}
    </button>
  );
}
