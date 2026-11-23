import { useMemo, useRef, useState } from "react";
import { Sociogram } from "sociogram/react";
import type { SociogramHandle } from "sociogram/react";
import { communityGraph } from "./graph";

export function App() {
  // Create the graph once; a new object would replace it on screen.
  const graph = useMemo(() => communityGraph(), []);
  const handle = useRef<SociogramHandle>(null);
  const [shown, setShown] = useState(true);
  const [status, setStatus] = useState("loading");
  const [clicks, setClicks] = useState(0);
  const [hover, setHover] = useState<number | null>(null);

  const toggle = () => {
    setStatus("loading"); // a new mount starts from scratch
    setShown((s) => !s);
  };

  return (
    <>
      <p>
        <button id="toggle" onClick={toggle}>
          {shown ? "Unmount" : "Mount"}
        </button>
        <button id="fit" onClick={() => handle.current?.fitView()}>Fit</button>
        <button id="reheat" onClick={() => handle.current?.reheat()}>Reheat</button>
        <span id="status" data-status={shown ? status : "unmounted"}>{shown ? status : "unmounted"}</span>{" "}
        clicks: <span id="clicks">{clicks}</span> hover: <span id="hover">{hover ?? "none"}</span>
      </p>
      {shown && (
        <div className="graph">
          <Sociogram
            ref={handle}
            data={graph}
            onReady={() => setStatus("ready")}
            onError={(error, phase) => setStatus(`error (${phase}): ${error.message}`)}
            onNodeClick={() => setClicks((c) => c + 1)}
            onNodeHover={setHover}
            fallback={(error) => <p style={{ padding: 16 }}>WebGPU is not available: {error.message}</p>}
          />
        </div>
      )}
    </>
  );
}
