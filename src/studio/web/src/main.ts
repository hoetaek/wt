import {
  ArrowsClockwise,
  CheckCircle,
  FileText,
  FloppyDisk,
  GearSix,
  List,
  Plus,
  WarningCircle
} from "@phosphor-icons/react";
import clsx from "clsx";
import { h, render, type ComponentChildren } from "preact";
import { useEffect, useMemo, useRef, useState } from "preact/hooks";
import {
  ConfigForm,
  configDraftEqual,
  configSummary,
  draftFromToml,
  emptyConfigDraft,
  serializeConfigDraft,
  type ConfigDraft
} from "./config-form";
import {
  ProfileForm,
  draftFromProfileToml,
  emptyProfileDraft,
  profileDraftEqual,
  profileSummary,
  serializeProfileDraft,
  type ProfileDraft
} from "./profile-form";
import { PromptEditor, promptModes, type PromptMode } from "./prompt-editor";
import { WorkflowView, type WorkflowDetail } from "./workflow-view";
import "./style.css";

type TaskOrigin = {
  provider: string;
  id: string;
};

type TaskDocument = {
  title: string;
  branch: string;
  body: string;
  origin?: TaskOrigin | null;
};

type Fingerprint = {
  mtime_ns: string | null;
  hash: string;
};

type TaskDocumentItem = {
  key: string;
  path: string;
  content: string;
  document: TaskDocument;
  fingerprint: Fingerprint;
};

type Inventory = {
  items: TaskDocumentItem[];
  invalid: Array<{ path: string; error: string }>;
};

type WorkflowListItem = {
  id: string;
  path: string;
  title?: string | null;
  mode: string;
  color?: string | null;
  updated_at: string;
};

type WorkflowInventory = {
  items: WorkflowListItem[];
};

type PlanResponse = {
  path: string;
  operation: "create" | "update";
  valid: boolean;
  validation_errors: string[];
  before: string;
  after: string;
  diff: string;
  precondition: Fingerprint;
};

type PersonalConfigPlanResponse = {
  before: string;
  after: string;
  diff: string;
  validation_errors: string[];
  fingerprint: Fingerprint;
  baseline_stale: boolean;
};

type ProfilePromptPlanResponse = PersonalConfigPlanResponse;

type ProfileItem = {
  name: string;
  path: string;
  has_profile_toml: boolean;
};

type ProfileInventory = {
  items: ProfileItem[];
};

type PreviewPlan = {
  path: string;
  operation: "create" | "update" | "config" | "prompt";
  valid: boolean;
  validation_errors: string[];
  diff: string;
};

type Mode = "create" | "update";
type SurfaceMode = "tasks" | "config" | "profiles" | "prompts" | "workflow";
type PlanStatus = "idle" | "planning" | "stale";

type EditorDraft = {
  slug: string;
  title: string;
  branch: string;
  body: string;
  originProvider: string;
  originId: string;
};

type IconComponent = (props: Record<string, unknown>) => ComponentChildren;
type DetailMetric = { label: string; value: string };

const StudioList = List as unknown as IconComponent;
const StudioPlus = Plus as unknown as IconComponent;
const StudioRefresh = ArrowsClockwise as unknown as IconComponent;
const StudioFile = FileText as unknown as IconComponent;
const StudioSave = FloppyDisk as unknown as IconComponent;
const StudioWarning = WarningCircle as unknown as IconComponent;
const StudioCheck = CheckCircle as unknown as IconComponent;
const StudioConfig = GearSix as unknown as IconComponent;
const PLAN_DEBOUNCE_MS = 500;

const emptyDraft: EditorDraft = {
  slug: "new-task",
  title: "",
  branch: "new-task",
  body: "",
  originProvider: "",
  originId: ""
};

const transition =
  "transition-[transform,opacity,background-color,color,box-shadow,filter] duration-700 ease-[cubic-bezier(0.32,0.72,0,1)]";
const eyebrow =
  "inline-flex w-fit items-center rounded-full bg-black/[0.04] px-3 py-1 text-[10px] font-medium uppercase tracking-[0.2em] text-neutral-500 ring-1 ring-black/5 dark:bg-white/[0.04] dark:text-neutral-400 dark:ring-white/10";

function App() {
  const initialPrompt = promptLocationFromHash();
  const [surface, setSurface] = useState<SurfaceMode>(() => surfaceFromHash());
  const [inventory, setInventory] = useState<Inventory>({ items: [], invalid: [] });
  const [workflows, setWorkflows] = useState<WorkflowInventory>({ items: [] });
  const [selectedWorkflowId, setSelectedWorkflowId] = useState("");
  const [workflowDetail, setWorkflowDetail] = useState<WorkflowDetail | null>(null);
  const [workflowLoading, setWorkflowLoading] = useState(false);
  const [mode, setMode] = useState<Mode>("create");
  const [selectedPath, setSelectedPath] = useState("");
  const [drawerOpen, setDrawerOpen] = useState(true);
  const [draft, setDraft] = useState<EditorDraft>(emptyDraft);
  const [plan, setPlan] = useState<PlanResponse | null>(null);
  const [planStatus, setPlanStatus] = useState<PlanStatus>("idle");
  const [baselineStale, setBaselineStale] = useState(false);
  const [configDraft, setConfigDraft] = useState<ConfigDraft>(() => emptyConfigDraft());
  const [configBaselineDraft, setConfigBaselineDraft] = useState<ConfigDraft>(() => emptyConfigDraft());
  const [configBaselineFingerprint, setConfigBaselineFingerprint] = useState<Fingerprint | null>(null);
  const [configPlan, setConfigPlan] = useState<PersonalConfigPlanResponse | null>(null);
  const [configPlanStatus, setConfigPlanStatus] = useState<PlanStatus>("idle");
  const [configLoaded, setConfigLoaded] = useState(false);
  const [profiles, setProfiles] = useState<ProfileInventory>({ items: [] });
  const [profilesLoaded, setProfilesLoaded] = useState(false);
  const [selectedProfileName, setSelectedProfileName] = useState(() => profileNameFromHash());
  const [profileDraft, setProfileDraft] = useState<ProfileDraft>(() => emptyProfileDraft());
  const [profileBaselineDraft, setProfileBaselineDraft] = useState<ProfileDraft>(() => emptyProfileDraft());
  const [profileBaselineFingerprint, setProfileBaselineFingerprint] = useState<Fingerprint | null>(null);
  const [profilePlan, setProfilePlan] = useState<PersonalConfigPlanResponse | null>(null);
  const [profilePlanStatus, setProfilePlanStatus] = useState<PlanStatus>("idle");
  const [profileLoadedName, setProfileLoadedName] = useState("");
  const [promptProfile, setPromptProfile] = useState(initialPrompt.profile);
  const [promptMode, setPromptMode] = useState<PromptMode>(initialPrompt.mode);
  const [promptCandidate, setPromptCandidate] = useState("");
  const [promptBaselineCandidate, setPromptBaselineCandidate] = useState("");
  const [promptBaselineFingerprint, setPromptBaselineFingerprint] = useState<Fingerprint | null>(null);
  const [promptPlan, setPromptPlan] = useState<ProfilePromptPlanResponse | null>(null);
  const [promptPlanStatus, setPromptPlanStatus] = useState<PlanStatus>("idle");
  const [promptLoadedKey, setPromptLoadedKey] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");

  const selected = useMemo(
    () => inventory.items.find((item) => item.path === selectedPath) || null,
    [inventory.items, selectedPath]
  );
  const draftIssues = useMemo(() => validateDraft(mode, draft), [draft, mode]);
  const currentPath = mode === "update" && selected ? selected.path : targetPath(mode, draft, selected);
  const cleanUpdateDraft =
    mode === "update" && selected && !baselineStale ? draftsEqual(draft, draftFromItem(selected)) : false;
  const planSignature = useMemo(() => planRequestSignature(mode, currentPath, draft), [currentPath, draft, mode]);
  const latestPlanSignature = useRef(planSignature);
  const configCandidate = useMemo(() => serializeConfigDraft(configDraft), [configDraft]);
  const configClean = configLoaded && configDraftEqual(configDraft, configBaselineDraft);
  const configPlanSignature = useMemo(
    () => JSON.stringify({ candidate: configCandidate, baseline: configBaselineFingerprint }),
    [configBaselineFingerprint, configCandidate]
  );
  const profileCandidate = useMemo(() => serializeProfileDraft(profileDraft), [profileDraft]);
  const profileClean =
    Boolean(selectedProfileName) &&
    profileLoadedName === selectedProfileName &&
    profileDraftEqual(profileDraft, profileBaselineDraft);
  const profilePlanSignature = useMemo(
    () =>
      JSON.stringify({
        profile: selectedProfileName,
        candidate: profileCandidate,
        baseline: profileBaselineFingerprint
      }),
    [profileBaselineFingerprint, profileCandidate, selectedProfileName]
  );
  const promptResourceKey = `${promptProfile.trim()}/${promptMode}`;
  const promptClean = promptLoadedKey === promptResourceKey && promptCandidate === promptBaselineCandidate;
  const promptPlanSignature = useMemo(
    () =>
      JSON.stringify({
        resource: promptResourceKey,
        candidate: promptCandidate,
        baseline: promptBaselineFingerprint
      }),
    [promptBaselineFingerprint, promptCandidate, promptResourceKey]
  );
  const latestConfigPlanSignature = useRef(configPlanSignature);
  const latestProfilePlanSignature = useRef(profilePlanSignature);
  const latestPromptPlanSignature = useRef(promptPlanSignature);
  const recoveryPlanController = useRef<AbortController | null>(null);
  const configPlanController = useRef<AbortController | null>(null);
  const profilePlanController = useRef<AbortController | null>(null);
  const promptPlanController = useRef<AbortController | null>(null);
  latestPlanSignature.current = planSignature;
  latestConfigPlanSignature.current = configPlanSignature;
  latestProfilePlanSignature.current = profilePlanSignature;
  latestPromptPlanSignature.current = promptPlanSignature;
  const activePlanStatus =
    surface === "prompts"
      ? promptPlanStatus
      : surface === "profiles"
        ? profilePlanStatus
        : surface === "config"
          ? configPlanStatus
          : planStatus;
  const activeStatus =
    surface === "prompts"
      ? promptStatusDescriptor(promptPlanStatus, promptPlan)
      : surface === "profiles"
        ? configStatusDescriptor(profilePlanStatus, profilePlan)
      : surface === "config"
        ? configStatusDescriptor(configPlanStatus, configPlan)
        : surface === "workflow"
          ? workflowStatusDescriptor(workflowLoading, workflowDetail)
        : statusDescriptor(planStatus, plan);
  const displaySlug =
    surface === "prompts"
      ? promptMode
      : surface === "profiles"
        ? selectedProfileName || "profile"
      : surface === "config"
        ? "local.toml"
        : surface === "workflow"
          ? selectedWorkflowId || "workflow"
        : mode === "create"
          ? draft.slug.trim() || "new-task"
          : selected?.key || draft.slug;
  const displayTitle =
    surface === "prompts"
      ? `${promptProfile.trim() || "profile"}/prompts/${promptMode}.md`
      : surface === "profiles"
        ? profileDisplayPath(selectedProfileName)
      : surface === "config"
        ? "Personal config"
        : surface === "workflow"
          ? workflowDetail?.title || "Workflow"
        : draft.title.trim() || "제목 없는 TaskDocument";
  const detailMetrics = useMemo(
    () =>
      surface === "prompts"
        ? buildPromptDetailMetrics(promptProfile, promptMode, promptCandidate, promptPlan, promptPlanStatus)
        : surface === "profiles"
        ? buildProfileDetailMetrics(selectedProfileName, profileCandidate, profileDraft, profilePlan, profilePlanStatus)
        : surface === "config"
        ? buildConfigDetailMetrics(configCandidate, configDraft, configPlan, configPlanStatus)
        : surface === "workflow"
        ? buildWorkflowDetailMetrics(workflowDetail, workflows.items.length)
        : buildDetailMetrics(currentPath, draft, selected, plan, draftIssues, planStatus),
    [
      configCandidate,
      configDraft,
      configPlan,
      configPlanStatus,
      currentPath,
      draft,
      draftIssues,
      plan,
      planStatus,
      profileCandidate,
      profileDraft,
      profilePlan,
      profilePlanStatus,
      promptCandidate,
      promptMode,
      promptPlan,
      promptPlanStatus,
      promptProfile,
      selectedProfileName,
      selected,
      surface,
      workflowDetail,
      workflows.items.length
    ]
  );

  useEffect(() => {
    void loadInventory();
    void loadWorkflows(workflowIdFromHash());
  }, []);

  useEffect(() => {
    const onHashChange = () => {
      const nextSurface = surfaceFromHash();
      setSurface(nextSurface);
      if (nextSurface === "prompts") {
        const nextPrompt = promptLocationFromHash();
        setPromptProfile(nextPrompt.profile);
        setPromptMode(nextPrompt.mode);
        setPromptLoadedKey("");
        resetPromptPlanState();
      } else if (nextSurface === "profiles") {
        setSelectedProfileName(profileNameFromHash());
        setProfileLoadedName("");
        resetProfilePlanState();
      }
      if (nextSurface === "workflow") {
        const id = workflowIdFromHash();
        if (id) {
          setSelectedWorkflowId(id);
          void loadWorkflowDetail(id);
        }
      }
    };
    window.addEventListener("hashchange", onHashChange);
    return () => window.removeEventListener("hashchange", onHashChange);
  }, []);

  useEffect(() => {
    if (surface !== "tasks") {
      return;
    }
    const hashPath = taskPathFromHash(inventory.items);
    if (hashPath && hashPath !== selectedPath) {
      selectUpdate(hashPath);
    }
  }, [inventory.items, selectedPath, surface]);

  useEffect(() => {
    if (surface === "config" && !configLoaded) {
      void loadPersonalConfig();
    }
  }, [configLoaded, surface]);

  useEffect(() => {
    if (surface === "profiles" && !profilesLoaded) {
      void loadProfiles();
    }
  }, [profilesLoaded, surface]);

  useEffect(() => {
    if (surface !== "profiles" || profiles.items.length === 0) {
      return;
    }
    const exists = profiles.items.some((item) => item.name === selectedProfileName);
    if (!selectedProfileName || !exists) {
      selectProfile(profiles.items[0].name);
    }
  }, [profiles.items, selectedProfileName, surface]);

  useEffect(() => {
    if (surface === "profiles" && selectedProfileName && profileLoadedName !== selectedProfileName) {
      void loadProfileConfig(selectedProfileName);
    }
  }, [profileLoadedName, selectedProfileName, surface]);

  useEffect(() => {
    if (surface === "prompts" && promptLoadedKey !== promptResourceKey) {
      void loadProfilePrompt();
    }
  }, [promptLoadedKey, promptResourceKey, surface]);

  useEffect(() => {
    if (surface !== "workflow") {
      return;
    }
    const id = workflowIdFromHash() || selectedWorkflowId || workflows.items[0]?.id || "";
    if (id && id !== selectedWorkflowId) {
      setSelectedWorkflowId(id);
      void loadWorkflowDetail(id);
    } else if (id && !workflowDetail) {
      void loadWorkflowDetail(id);
    }
  }, [selectedWorkflowId, surface, workflowDetail, workflows.items]);

  useEffect(() => {
    return () => {
      recoveryPlanController.current?.abort();
      recoveryPlanController.current = null;
      configPlanController.current?.abort();
      configPlanController.current = null;
      profilePlanController.current?.abort();
      profilePlanController.current = null;
      promptPlanController.current?.abort();
      promptPlanController.current = null;
    };
  }, []);

  useEffect(() => {
    if (surface !== "tasks") {
      return;
    }
    if (mode === "update" && !selected) {
      resetPlanState();
      return;
    }
    if (cleanUpdateDraft || draftIssues.length > 0) {
      resetPlanState();
      return;
    }

    const controller = new AbortController();
    const signature = planSignature;
    const timer = window.setTimeout(() => {
      if (latestPlanSignature.current !== signature) {
        return;
      }
      recoveryPlanController.current?.abort();
      recoveryPlanController.current = null;
      setPlanStatus("planning");
      setError("");
      void planDraft(controller.signal, signature);
    }, PLAN_DEBOUNCE_MS);

    return () => {
      window.clearTimeout(timer);
      controller.abort();
    };
  }, [cleanUpdateDraft, draftIssues.length, mode, planSignature, selected, surface]);

  useEffect(() => {
    if (surface !== "config" || !configLoaded) {
      return;
    }
    if (configClean) {
      resetConfigPlanState();
      return;
    }

    const controller = new AbortController();
    const signature = configPlanSignature;
    const timer = window.setTimeout(() => {
      if (latestConfigPlanSignature.current !== signature) {
        return;
      }
      configPlanController.current?.abort();
      configPlanController.current = controller;
      setConfigPlanStatus("planning");
      setError("");
      void planConfigDraft(controller.signal, signature);
    }, PLAN_DEBOUNCE_MS);

    return () => {
      window.clearTimeout(timer);
      controller.abort();
    };
  }, [configClean, configLoaded, configPlanSignature, surface]);

  useEffect(() => {
    if (surface !== "profiles" || !selectedProfileName || profileLoadedName !== selectedProfileName) {
      return;
    }
    if (profileClean) {
      resetProfilePlanState();
      return;
    }

    const controller = new AbortController();
    const signature = profilePlanSignature;
    const timer = window.setTimeout(() => {
      if (latestProfilePlanSignature.current !== signature) {
        return;
      }
      profilePlanController.current?.abort();
      profilePlanController.current = controller;
      setProfilePlanStatus("planning");
      setError("");
      void planProfileDraft(controller.signal, signature);
    }, PLAN_DEBOUNCE_MS);

    return () => {
      window.clearTimeout(timer);
      controller.abort();
    };
  }, [profileClean, profileLoadedName, profilePlanSignature, selectedProfileName, surface]);

  useEffect(() => {
    if (surface !== "prompts" || promptLoadedKey !== promptResourceKey) {
      return;
    }
    if (promptClean) {
      resetPromptPlanState();
      return;
    }

    const controller = new AbortController();
    const signature = promptPlanSignature;
    const timer = window.setTimeout(() => {
      if (latestPromptPlanSignature.current !== signature) {
        return;
      }
      promptPlanController.current?.abort();
      promptPlanController.current = controller;
      setPromptPlanStatus("planning");
      setError("");
      void planProfilePrompt(controller.signal, signature);
    }, PLAN_DEBOUNCE_MS);

    return () => {
      window.clearTimeout(timer);
      controller.abort();
    };
  }, [promptClean, promptLoadedKey, promptPlanSignature, promptResourceKey, surface]);

  useEffect(() => {
    const nodes = Array.from(document.querySelectorAll<HTMLElement>("[data-reveal]"));
    if (!("IntersectionObserver" in window)) {
      nodes.forEach((node) => node.setAttribute("data-reveal", "visible"));
      return;
    }
    const observer = new IntersectionObserver(
      (entries) => {
        entries.forEach((entry) => {
          if (entry.isIntersecting) {
            entry.target.setAttribute("data-reveal", "visible");
            observer.unobserve(entry.target);
          }
        });
      },
      { threshold: 0.16 }
    );
    nodes.forEach((node) => observer.observe(node));
    return () => observer.disconnect();
  }, [inventory.items.length, profiles.items.length, plan?.diff, profilePlan?.diff, promptPlan?.diff, error]);

  async function loadInventory(nextSelectedPath?: string, modeOverride?: Mode) {
    setBusy(true);
    setError("");
    try {
      const next = await api<Inventory>("/api/task-documents", { method: "POST" });
      setInventory(next);
      setBaselineStale(false);
      const fallbackPath = next.items[0]?.path || "";
      const hashPath = taskPathFromHash(next.items);
      const resolvedPath = nextSelectedPath ?? (selectedPath || hashPath);
      const nextPath = next.items.some((item) => item.path === resolvedPath) ? resolvedPath : fallbackPath;
      setSelectedPath(nextPath);
      const loadMode = modeOverride ?? mode;
      if (loadMode === "update") {
        const nextSelected = next.items.find((item) => item.path === nextPath);
        if (nextSelected) {
          setDraft(draftFromItem(nextSelected));
        }
      }
    } catch (err) {
      setError(errorMessage(err));
    } finally {
      setBusy(false);
    }
  }

  async function loadPersonalConfig() {
    setBusy(true);
    setError("");
    try {
      const response = await api<PersonalConfigPlanResponse>("/api/personal-config/plan", {
        method: "POST",
        body: JSON.stringify({
          candidate: "",
          baseline_fingerprint: null
        })
      });
      const loadedDraft = draftFromToml(response.before);
      setConfigDraft(loadedDraft);
      setConfigBaselineDraft(loadedDraft);
      setConfigBaselineFingerprint(response.fingerprint);
      setConfigPlan(null);
      setConfigPlanStatus("idle");
      setConfigLoaded(true);
    } catch (err) {
      setError(errorMessage(err));
    } finally {
      setBusy(false);
    }
  }

  async function loadProfiles(nextProfileName?: string) {
    setBusy(true);
    setError("");
    try {
      const next = await api<ProfileInventory>("/api/profiles", { method: "GET" });
      setProfiles(next);
      setProfilesLoaded(true);
      const hashProfile = profileNameFromHash();
      const requested = nextProfileName || selectedProfileName || hashProfile;
      const fallback = next.items[0]?.name || "";
      const resolved = next.items.some((item) => item.name === requested) ? requested : fallback;
      if (resolved && resolved !== selectedProfileName) {
        setSelectedProfileName(resolved);
        window.location.hash = `profile/${encodeURIComponent(resolved)}`;
      }
    } catch (err) {
      setError(errorMessage(err));
    } finally {
      setBusy(false);
    }
  }

  async function loadProfileConfig(name: string) {
    setBusy(true);
    setError("");
    try {
      const response = await api<PersonalConfigPlanResponse>(profileConfigApiPath(name, "plan"), {
        method: "POST",
        body: JSON.stringify({
          candidate: "",
          baseline_fingerprint: null
        })
      });
      const loadedDraft = draftFromProfileToml(response.before);
      setProfileDraft(loadedDraft);
      setProfileBaselineDraft(loadedDraft);
      setProfileBaselineFingerprint(response.fingerprint);
      setProfilePlan(null);
      setProfilePlanStatus("idle");
      setProfileLoadedName(name);
    } catch (err) {
      setError(errorMessage(err));
      setProfileLoadedName(name);
      setProfileDraft(emptyProfileDraft());
      setProfileBaselineDraft(emptyProfileDraft());
      setProfileBaselineFingerprint(null);
      setProfilePlan(null);
      setProfilePlanStatus("idle");
    } finally {
      setBusy(false);
    }
  }

  async function loadProfilePrompt() {
    setBusy(true);
    setError("");
    try {
      const response = await api<ProfilePromptPlanResponse>(profilePromptApiPath("plan"), {
        method: "POST",
        body: JSON.stringify({
          candidate: "",
          baseline_fingerprint: null
        })
      });
      setPromptCandidate(response.before);
      setPromptBaselineCandidate(response.before);
      setPromptBaselineFingerprint(response.fingerprint);
      setPromptPlan(null);
      setPromptPlanStatus("idle");
      setPromptLoadedKey(promptResourceKey);
    } catch (err) {
      setError(errorMessage(err));
      setPromptLoadedKey(promptResourceKey);
      setPromptCandidate("");
      setPromptBaselineCandidate("");
      setPromptBaselineFingerprint(null);
      setPromptPlan(null);
      setPromptPlanStatus("idle");
    } finally {
      setBusy(false);
    }
  }

  async function loadWorkflows(nextSelectedId?: string) {
    setError("");
    try {
      const next = await api<WorkflowInventory>("/api/workflows", { method: "GET" });
      setWorkflows(next);
      const id = nextSelectedId || selectedWorkflowId || next.items[0]?.id || "";
      if (id && next.items.some((item) => item.id === id)) {
        setSelectedWorkflowId(id);
        if (surface === "workflow" || window.location.hash.startsWith("#workflow/")) {
          await loadWorkflowDetail(id);
        }
      }
    } catch (err) {
      setError(errorMessage(err));
    }
  }

  async function loadWorkflowDetail(id: string) {
    setWorkflowLoading(true);
    setError("");
    try {
      const detail = await api<WorkflowDetail>(`/api/workflows/${encodeURIComponent(id)}`, { method: "GET" });
      setWorkflowDetail(detail);
    } catch (err) {
      setWorkflowDetail(null);
      setError(errorMessage(err));
    } finally {
      setWorkflowLoading(false);
    }
  }

  function switchSurface(next: SurfaceMode) {
    setSurface(next);
    setError("");
    if (next === "config") {
      window.location.hash = "config";
    } else if (next === "profiles") {
      const name = selectedProfileName || profiles.items[0]?.name || "";
      window.location.hash = name ? `profile/${encodeURIComponent(name)}` : "profile";
    } else if (next === "prompts") {
      window.location.hash = promptHash(promptProfile, promptMode);
    } else if (next === "workflow") {
      const id = selectedWorkflowId || workflows.items[0]?.id || "";
      window.location.hash = id ? `workflow/${encodeURIComponent(id)}` : "workflow";
      if (id) {
        void loadWorkflowDetail(id);
      }
    } else {
      const slug = mode === "update" && selected ? selected.key : draft.slug.trim() || "new-task";
      window.location.hash = `task/${encodeURIComponent(slug)}`;
    }
  }

  function selectWorkflow(id: string) {
    setSurface("workflow");
    setSelectedWorkflowId(id);
    setError("");
    window.location.hash = `workflow/${encodeURIComponent(id)}`;
    void loadWorkflowDetail(id);
  }

  function selectCreate() {
    setSurface("tasks");
    window.location.hash = "task/new-task";
    setMode("create");
    setBaselineStale(false);
    resetPlanState();
    setError("");
    setDraft(emptyDraft);
  }

  function selectUpdate(path: string) {
    const item = inventory.items.find((candidate) => candidate.path === path);
    if (!item) return;
    setSurface("tasks");
    window.location.hash = `task/${encodeURIComponent(item.key)}`;
    setMode("update");
    setSelectedPath(path);
    setBaselineStale(false);
    resetPlanState();
    setError("");
    setDraft(draftFromItem(item));
  }

  function resetPlanState() {
    recoveryPlanController.current?.abort();
    recoveryPlanController.current = null;
    setPlan(null);
    setPlanStatus("idle");
  }

  function resetConfigPlanState() {
    configPlanController.current?.abort();
    configPlanController.current = null;
    setConfigPlan(null);
    setConfigPlanStatus("idle");
  }

  function resetProfilePlanState() {
    profilePlanController.current?.abort();
    profilePlanController.current = null;
    setProfilePlan(null);
    setProfilePlanStatus("idle");
  }

  function resetPromptPlanState() {
    promptPlanController.current?.abort();
    promptPlanController.current = null;
    setPromptPlan(null);
    setPromptPlanStatus("idle");
  }

  function triggerConflictRecoveryPlan() {
    recoveryPlanController.current?.abort();
    const controller = new AbortController();
    recoveryPlanController.current = controller;
    const signature = latestPlanSignature.current;
    setPlanStatus("planning");
    void planDraft(controller.signal, signature, { failureStatus: "stale" }).finally(() => {
      if (recoveryPlanController.current === controller) {
        recoveryPlanController.current = null;
      }
    });
  }

  async function planDraft(
    signal: AbortSignal,
    signature: string,
    options: { failureStatus?: PlanStatus } = {}
  ) {
    try {
      const response = await api<PlanResponse>("/api/task-documents/plan", {
        method: "POST",
        signal,
        body: JSON.stringify({
          path: targetPath(mode, draft, selected),
          mode,
          document: documentFromDraft(draft)
        })
      });
      if (latestPlanSignature.current !== signature) {
        return;
      }
      setPlan(response);
      setError("");
      setPlanStatus("idle");
    } catch (err) {
      if (isAbortError(err) || latestPlanSignature.current !== signature) {
        return;
      }
      setError(errorMessage(err));
      setPlan(null);
      setPlanStatus(options.failureStatus ?? "idle");
    }
  }

  async function planConfigDraft(signal: AbortSignal, signature: string) {
    try {
      const response = await api<PersonalConfigPlanResponse>("/api/personal-config/plan", {
        method: "POST",
        signal,
        body: JSON.stringify({
          candidate: configCandidate,
          baseline_fingerprint: configBaselineFingerprint
        })
      });
      if (latestConfigPlanSignature.current !== signature) {
        return;
      }
      setConfigPlan(response);
      setError("");
      setConfigPlanStatus(response.baseline_stale ? "stale" : "idle");
    } catch (err) {
      if (isAbortError(err) || latestConfigPlanSignature.current !== signature) {
        return;
      }
      setError(errorMessage(err));
      setConfigPlan(null);
      setConfigPlanStatus("idle");
    }
  }

  async function planProfileDraft(signal: AbortSignal, signature: string) {
    try {
      const response = await api<PersonalConfigPlanResponse>(profileConfigApiPath(selectedProfileName, "plan"), {
        method: "POST",
        signal,
        body: JSON.stringify({
          candidate: profileCandidate,
          baseline_fingerprint: profileBaselineFingerprint
        })
      });
      if (latestProfilePlanSignature.current !== signature) {
        return;
      }
      setProfilePlan(response);
      setError("");
      setProfilePlanStatus(response.baseline_stale ? "stale" : "idle");
    } catch (err) {
      if (isAbortError(err) || latestProfilePlanSignature.current !== signature) {
        return;
      }
      setError(errorMessage(err));
      setProfilePlan(null);
      setProfilePlanStatus("idle");
    }
  }

  async function planProfilePrompt(signal: AbortSignal, signature: string) {
    try {
      const response = await api<ProfilePromptPlanResponse>(profilePromptApiPath("plan"), {
        method: "POST",
        signal,
        body: JSON.stringify({
          candidate: promptCandidate,
          baseline_fingerprint: promptBaselineFingerprint
        })
      });
      if (latestPromptPlanSignature.current !== signature) {
        return;
      }
      setPromptPlan(response);
      setError("");
      setPromptPlanStatus(response.baseline_stale ? "stale" : "idle");
    } catch (err) {
      if (isAbortError(err) || latestPromptPlanSignature.current !== signature) {
        return;
      }
      setError(errorMessage(err));
      setPromptPlan(null);
      setPromptPlanStatus("idle");
    }
  }

  async function applyPlan() {
    if (!plan || !plan.valid) return;
    setBusy(true);
    setError("");
    try {
      await api("/api/task-documents/apply", {
        method: "POST",
        body: JSON.stringify({
          path: plan.path,
          before: plan.before,
          after: plan.after,
          precondition: plan.precondition
        })
      });
      setPlan(null);
      setPlanStatus("idle");
      setMode("update");
      setSelectedPath(plan.path);
      await loadInventory(plan.path, "update");
    } catch (err) {
      const apiErr = err as ApiFailure;
      setError(apiErr.diff ? `${apiErr.message}\n\n${apiErr.diff}` : errorMessage(err));
      if (apiErr.status === 409) {
        setBaselineStale(true);
        setPlan(null);
        triggerConflictRecoveryPlan();
      }
    } finally {
      setBusy(false);
    }
  }

  async function applyConfigPlan() {
    if (!configPlan || configPlan.validation_errors.length > 0 || configPlan.baseline_stale) return;
    setBusy(true);
    setError("");
    try {
      const applied = await api<{ committed_fingerprint: Fingerprint }>("/api/personal-config/apply", {
        method: "POST",
        body: JSON.stringify({
          candidate: configCandidate,
          precondition: configPlan.fingerprint
        })
      });
      setConfigBaselineDraft(configDraft);
      setConfigBaselineFingerprint(applied.committed_fingerprint);
      setConfigPlan(null);
      setConfigPlanStatus("idle");
    } catch (err) {
      const apiErr = err as ApiFailure;
      setError(errorMessage(err));
      if (apiErr.status === 409) {
        setConfigPlanStatus("stale");
      }
    } finally {
      setBusy(false);
    }
  }

  async function applyProfilePlan() {
    if (!profilePlan || profilePlan.validation_errors.length > 0 || profilePlan.baseline_stale) return;
    setBusy(true);
    setError("");
    try {
      const applied = await api<{ committed_fingerprint: Fingerprint }>(profileConfigApiPath(selectedProfileName, "apply"), {
        method: "POST",
        body: JSON.stringify({
          candidate: profileCandidate,
          precondition: profilePlan.fingerprint
        })
      });
      setProfileBaselineDraft(profileDraft);
      setProfileBaselineFingerprint(applied.committed_fingerprint);
      setProfilePlan(null);
      setProfilePlanStatus("idle");
      await loadProfiles(selectedProfileName);
    } catch (err) {
      const apiErr = err as ApiFailure;
      setError(errorMessage(err));
      if (apiErr.status === 409) {
        setProfilePlanStatus("stale");
      }
    } finally {
      setBusy(false);
    }
  }

  async function applyPromptPlan() {
    if (!promptPlan || promptPlan.validation_errors.length > 0 || promptPlan.baseline_stale) return;
    setBusy(true);
    setError("");
    try {
      const applied = await api<{ committed_fingerprint: Fingerprint }>(profilePromptApiPath("apply"), {
        method: "POST",
        body: JSON.stringify({
          candidate: promptCandidate,
          precondition: promptPlan.fingerprint
        })
      });
      setPromptBaselineCandidate(promptCandidate);
      setPromptBaselineFingerprint(applied.committed_fingerprint);
      setPromptPlan(null);
      setPromptPlanStatus("idle");
    } catch (err) {
      const apiErr = err as ApiFailure;
      setError(errorMessage(err));
      if (apiErr.status === 409) {
        setPromptPlanStatus("stale");
      }
    } finally {
      setBusy(false);
    }
  }

  function updateDraft(key: keyof EditorDraft, value: string) {
    setDraft((current) => {
      const next = { ...current, [key]: value };
      if (key === "slug" && mode === "create") {
        next.branch = value;
      }
      return next;
    });
    resetPlanState();
  }

  function selectProfile(name: string) {
    setSurface("profiles");
    setSelectedProfileName(name);
    setProfileLoadedName("");
    resetProfilePlanState();
    setError("");
    window.location.hash = `profile/${encodeURIComponent(name)}`;
  }

  function updatePromptProfile(profile: string) {
    setPromptProfile(profile);
    setPromptLoadedKey("");
    resetPromptPlanState();
    window.location.hash = promptHash(profile, promptMode);
  }

  function updatePromptMode(nextMode: PromptMode) {
    setPromptMode(nextMode);
    setPromptLoadedKey("");
    resetPromptPlanState();
    window.location.hash = promptHash(promptProfile, nextMode);
  }

  function updatePromptCandidate(value: string) {
    setPromptCandidate(value);
    resetPromptPlanState();
  }

  function profilePromptApiPath(action: "plan" | "apply") {
    return `/api/profile-prompts/${encodeURIComponent(promptProfile.trim())}/${promptMode}/${action}`;
  }

  function profileConfigApiPath(name: string, action: "plan" | "apply") {
    return `/api/profiles/${encodeURIComponent(name)}/${action}`;
  }

  return h("main", { class: "relative min-h-[100dvh] overflow-hidden px-4 py-12 text-neutral-950 dark:text-neutral-50 sm:py-16" }, [
    h("div", { class: "studio-noise", "aria-hidden": "true" }),
    h(
      "header",
      {
        class:
          "sticky top-4 z-20 mx-auto mb-12 flex w-full max-w-7xl items-center justify-between rounded-full bg-white/70 px-3 py-3 shadow-[0_20px_50px_-28px_rgba(0,0,0,0.28)] ring-1 ring-black/5 backdrop-blur-xl dark:bg-neutral-950/70 dark:ring-white/10 md:mb-16",
        "data-reveal": ""
      },
      [
        h("button", {
          type: "button",
          "aria-label": drawerOpen ? "TaskDocument 목록 숨기기" : "TaskDocument 목록 보이기",
          onClick: () => setDrawerOpen((open) => !open),
          class: clsx(
            "group flex h-12 w-12 items-center justify-center rounded-full bg-black/[0.04] text-neutral-700 ring-1 ring-black/5 active:scale-[0.98] dark:bg-white/[0.08] dark:text-neutral-200 dark:ring-white/10",
            transition
          )
        }, icon(StudioList, "h-5 w-5 group-hover:translate-x-1 group-hover:-translate-y-px " + transition)),
        h("div", { class: "text-center" }, [
          h("p", { class: "text-[10px] font-medium uppercase tracking-[0.2em] text-neutral-500 dark:text-neutral-400" }, "wt studio"),
          h("div", { class: "mt-1 inline-flex rounded-full bg-black/[0.04] p-1 ring-1 ring-black/5 dark:bg-white/[0.05] dark:ring-white/10" }, [
            h(SurfacePill, { label: "Tasks", active: surface === "tasks", onClick: () => switchSurface("tasks") }),
            h(SurfacePill, { label: "Personal config", active: surface === "config", onClick: () => switchSurface("config") }),
            h(SurfacePill, { label: "Profiles", active: surface === "profiles", onClick: () => switchSurface("profiles") }),
            h(SurfacePill, { label: "Prompts", active: surface === "prompts", onClick: () => switchSurface("prompts") }),
            h(SurfacePill, { label: "Workflows", active: surface === "workflow", onClick: () => switchSurface("workflow") })
          ])
        ]),
        h("div", { class: "flex items-center gap-2" }, [
          surface === "tasks" && h(IconButton, { label: "새로 만들기", iconComponent: StudioPlus, onClick: selectCreate }),
          h(IconButton, {
            label: "새로고침",
            iconComponent: StudioRefresh,
            onClick: () =>
              surface === "config"
                ? void loadPersonalConfig()
                : surface === "profiles"
                  ? void loadProfiles()
                  : surface === "prompts"
                    ? void loadProfilePrompt()
                    : surface === "workflow"
                      ? void loadWorkflows(selectedWorkflowId)
                    : void loadInventory(),
            disabled: busy
          })
        ])
      ]
    ),
    h("section", { class: "mx-auto grid max-w-7xl gap-10 md:grid-cols-12 md:items-start", "data-reveal": "" }, [
      h("aside", { class: "md:col-span-5" }, [
        h("div", { class: "flex min-h-[42rem] flex-col justify-between gap-12 py-4" }, [
          h("div", { class: "space-y-8" }, [
            h(
              "p",
              { class: eyebrow },
              surface === "config"
                ? "Personal config"
                : surface === "profiles"
                  ? "Profile config"
                  : surface === "prompts"
                    ? "Profile prompt"
                    : surface === "workflow"
                      ? "Workflow"
                    : mode === "create"
                      ? "새 초안"
                      : "선택됨"
            ),
            h("div", {}, [
              h(
                "h1",
                {
                  class:
                    "max-w-[10ch] [overflow-wrap:anywhere] text-[clamp(3rem,6vw,6rem)] font-medium leading-[0.95] tracking-normal text-neutral-950 dark:text-neutral-50"
                },
                displaySlug
              ),
              h("p", { class: "mt-6 max-w-[34ch] text-base leading-7 text-neutral-500 dark:text-neutral-400" }, displayTitle)
            ]),
            h("div", { class: "flex flex-wrap gap-2" }, [
              h(MetaPill, { label: surface === "prompts" ? "Prompt Plan" : surface === "config" ? "Config Plan" : surface === "workflow" ? "Read-only" : mode === "create" ? "생성 Plan" : "수정 Plan" }),
              h(MetaPill, {
                label:
                  surface === "prompts"
                    ? promptPlanSummaryLabel(promptPlanStatus, promptPlan)
                    : surface === "profiles"
                      ? configPlanSummaryLabel(profilePlanStatus, profilePlan)
                    : surface === "config"
                      ? configPlanSummaryLabel(configPlanStatus, configPlan)
                      : surface === "workflow"
                        ? workflowSummaryLabel(workflowLoading, workflowDetail)
                      : planSummaryLabel(planStatus, plan),
                tone:
                  surface === "workflow" && workflowDetail
                    ? "blue"
                    : activePlanStatus === "stale"
                    ? "amber"
                    : activePlanStatus === "planning" ||
                        (surface === "prompts"
                          ? promptPlan && promptPlan.validation_errors.length === 0
                          : surface === "profiles"
                            ? profilePlan && profilePlan.validation_errors.length === 0
                          : surface === "config"
                            ? configPlan && configPlan.validation_errors.length === 0
                            : plan?.valid)
                      ? "blue"
                      : "neutral"
              }),
              surface === "tasks" && inventory.invalid.length > 0 && h(MetaPill, { label: `오류 ${inventory.invalid.length}개`, tone: "amber" })
            ]),
            h("dl", { class: "grid gap-4 text-sm text-neutral-500 dark:text-neutral-400" }, detailMetrics.map((item) => metric(item.label, item.value)))
          ]),
          drawerOpen &&
            h(Bezel, { className: "animate-[studio-spring_700ms_var(--ease-studio)_both]" }, [
              surface === "prompts"
                ? h(PromptResourceList, { profile: promptProfile, mode: promptMode, onModeChange: updatePromptMode })
                : surface === "profiles"
                ? h(ProfileResourceList, { profiles, selectedName: selectedProfileName, onSelect: selectProfile })
                : surface === "config"
                ? h(ConfigResourceList, { selected: true })
                : surface === "workflow"
                ? h(WorkflowList, { workflows, selectedId: selectedWorkflowId, selectWorkflow })
                : [
                    h("div", { class: "flex items-center justify-between gap-4" }, [
                      h("div", {}, [
                        h("p", { class: eyebrow }, "디스크"),
                        h("p", { class: "mt-3 text-2xl font-medium text-neutral-950 dark:text-neutral-50" }, `${inventory.items.length} TaskDocuments`)
                      ]),
                      h("button", {
                        type: "button",
                        onClick: selectCreate,
                        class: clsx(
                          "group flex h-12 w-12 items-center justify-center rounded-full bg-studio-accent text-white active:scale-[0.98]",
                          transition
                        )
                      }, icon(StudioPlus, "h-5 w-5 group-hover:translate-x-1 group-hover:-translate-y-px " + transition))
                    ]),
                    h(TaskList, { inventory, mode, selectedPath, selectUpdate })
                  ]
            ])
        ])
      ]),
      h("section", { class: "grid gap-8 md:col-span-7", "aria-label": surface === "prompts" ? "Profile prompt editor" : surface === "profiles" ? "Profile config editor" : surface === "config" ? "Personal config editor" : surface === "workflow" ? "Workflow inspector" : "TaskDocument editor" }, [
        h(Bezel, {}, [
          h("div", { class: "flex flex-col gap-6 md:flex-row md:items-center md:justify-between" }, [
            h("div", {}, [
              h("p", { class: eyebrow }, "편집기"),
              h(
                "h2",
                { class: "mt-4 text-3xl font-medium tracking-normal text-neutral-950 dark:text-neutral-50" },
                surface === "prompts" ? "Prompt Markdown Plan" : surface === "profiles" ? "profile.toml Plan" : surface === "config" ? "local.toml Plan" : surface === "workflow" ? "Workflow Inspector" : "Apply 전 Plan"
              )
            ]),
            h("span", { "aria-live": "polite", "aria-atomic": "true" }, [
              h("span", { class: statusClass(activeStatus.tone, activeStatus.pulse) }, activeStatus.label)
            ])
          ]),
          surface === "workflow"
            ? h(WorkflowView, { workflow: workflowDetail, loading: workflowLoading, iconComponent: StudioFile })
            : surface === "prompts"
            ? h(PromptEditor, {
                profile: promptProfile,
                mode: promptMode,
                value: promptCandidate,
                iconComponent: StudioFile,
                onProfileChange: updatePromptProfile,
                onModeChange: updatePromptMode,
                onChange: updatePromptCandidate
              })
            : surface === "profiles"
            ? selectedProfileName
              ? h(ProfileForm, {
                  draft: profileDraft,
                  iconComponent: StudioConfig,
                  onChange: (nextDraft: ProfileDraft) => {
                    setProfileDraft(nextDraft);
                    resetProfilePlanState();
                  }
                })
              : h("p", { class: "text-sm leading-6 text-neutral-500 dark:text-neutral-400" }, "profile.toml이 있는 profile이 없습니다.")
            : surface === "config"
            ? h(ConfigForm, {
                draft: configDraft,
                iconComponent: StudioConfig,
                onChange: (nextDraft: ConfigDraft) => {
                  setConfigDraft(nextDraft);
                  resetConfigPlanState();
                }
              })
            : [
                h("div", { class: "grid gap-4 md:grid-cols-2" }, [
                  h(Field, {
                    name: "slug",
                    label: "Slug",
                    value: draft.slug,
                    onInput: (value: string) => updateDraft("slug", value),
                    disabled: mode === "update"
                  }),
                  h(Field, { name: "title", label: "Title", value: draft.title, onInput: (value: string) => updateDraft("title", value) }),
                  h(Field, { name: "branch", label: "Branch", value: draft.branch, onInput: (value: string) => updateDraft("branch", value) }),
                  h(Field, {
                    name: "origin-provider",
                    label: "Origin provider",
                    value: draft.originProvider,
                    onInput: (value: string) => updateDraft("originProvider", value)
                  }),
                  h(Field, {
                    name: "origin-id",
                    label: "Origin id",
                    value: draft.originId,
                    onInput: (value: string) => updateDraft("originId", value),
                    className: "md:col-span-2"
                  })
                ]),
                h("label", { class: "grid gap-2 text-sm font-medium text-neutral-600 dark:text-neutral-300" }, [
                  h("span", {}, "Body"),
                  h("textarea", {
                    id: "task-document-body",
                    name: "body",
                    value: draft.body,
                    onInput: (event: Event) => updateDraft("body", formValue(event)),
                    rows: 13,
                    class: clsx(
                      "min-h-[22rem] rounded-[1.5rem] bg-white/70 px-5 py-4 font-mono text-sm leading-6 text-neutral-950 shadow-[inset_0_1px_1px_rgba(255,255,255,0.75)] ring-1 ring-black/5 placeholder:text-neutral-400 focus:ring-2 focus:ring-studio-accent/50 dark:bg-neutral-900/70 dark:text-neutral-50 dark:ring-white/10",
                      transition
                    )
                  })
                ]),
                draftIssues.length > 0 && h(ValidationList, { items: draftIssues })
              ],
          surface !== "workflow" &&
            h("div", { class: "flex flex-col gap-3 pt-2 sm:flex-row" }, [
              h(ActionButton, {
                label: "Apply",
                iconComponent: StudioSave,
                onClick: surface === "prompts" ? applyPromptPlan : surface === "profiles" ? applyProfilePlan : surface === "config" ? applyConfigPlan : applyPlan,
                disabled:
                  surface === "prompts"
                    ? busy || promptPlanStatus === "planning" || !promptPlan || promptPlan.validation_errors.length > 0 || promptPlan.baseline_stale
                    : surface === "profiles"
                      ? busy || profilePlanStatus === "planning" || !profilePlan || profilePlan.validation_errors.length > 0 || profilePlan.baseline_stale
                    : surface === "config"
                    ? busy || configPlanStatus === "planning" || !configPlan || configPlan.validation_errors.length > 0 || configPlan.baseline_stale
                    : busy || planStatus === "planning" || !plan?.valid,
                tone: "primary"
              })
            ])
        ]),
        error && h(MessagePanel, { message: error, tone: "error" }),
        surface === "workflow"
          ? null
          : surface === "prompts"
          ? promptPlan && h(PlanPreview, { plan: profilePromptPreview(promptProfile, promptMode, promptPlan) })
          : surface === "profiles"
          ? profilePlan && h(PlanPreview, { plan: profileConfigPreview(selectedProfileName, profilePlan) })
          : surface === "config"
          ? configPlan && h(PlanPreview, { plan: personalConfigPreview(configPlan) })
          : plan && h(PlanPreview, { plan: taskPreview(plan) })
      ])
    ])
  ]);
}

function TaskList(props: {
  inventory: Inventory;
  mode: Mode;
  selectedPath: string;
  selectUpdate: (path: string) => void;
}) {
  if (props.inventory.items.length === 0) {
    return h("p", { class: "mt-6 text-sm leading-6 text-neutral-500 dark:text-neutral-400" }, "디스크에 TaskDocument가 없습니다.");
  }
  return h("div", { class: "mt-6 grid max-h-[24rem] gap-2 overflow-auto pr-1" }, [
    ...props.inventory.items.map((item) => {
      const selected = props.mode === "update" && item.path === props.selectedPath;
      return h(
        "button",
        {
          key: item.path,
          type: "button",
          onClick: () => props.selectUpdate(item.path),
          class: clsx(
            "group grid w-full gap-1 rounded-[1.25rem] px-4 py-3 text-left ring-1 active:scale-[0.98]",
            selected
              ? "bg-studio-accent text-white ring-studio-accent"
              : "bg-white/40 text-neutral-700 ring-black/5 hover:bg-white/80 dark:bg-white/[0.03] dark:text-neutral-200 dark:ring-white/10 dark:hover:bg-white/[0.06]",
            transition
          )
        },
        [
          h("span", { class: "truncate text-sm font-medium" }, item.document.title || item.key),
          h("span", { class: clsx("truncate text-xs", selected ? "text-white/70" : "text-neutral-500 dark:text-neutral-500") }, item.document.branch || item.key)
        ]
      );
    }),
    props.inventory.invalid.length > 0 &&
      h("div", { class: "mt-4 rounded-[1.25rem] bg-amber-400/10 p-4 text-sm text-amber-700 ring-1 ring-amber-500/20 dark:text-amber-300" }, [
        h("div", { class: "flex items-center gap-2 font-medium" }, [icon(StudioWarning, "h-4 w-4"), "오류 TaskDocuments"]),
        ...props.inventory.invalid.map((item) =>
          h("p", { key: item.path, class: "mt-2 [overflow-wrap:anywhere] text-xs leading-5" }, `${item.path}: ${item.error}`)
        )
      ])
  ]);
}

function WorkflowList(props: {
  workflows: WorkflowInventory;
  selectedId: string;
  selectWorkflow: (id: string) => void;
}) {
  if (props.workflows.items.length === 0) {
    return h("p", { class: "mt-6 text-sm leading-6 text-neutral-500 dark:text-neutral-400" }, "디스크에 Workflow가 없습니다.");
  }
  return h("div", { class: "grid gap-5" }, [
    h("div", {}, [
      h("p", { class: eyebrow }, "디스크"),
      h("p", { class: "mt-3 text-2xl font-medium text-neutral-950 dark:text-neutral-50" }, `${props.workflows.items.length} Workflows`)
    ]),
    h(
      "div",
      { class: "grid max-h-[24rem] gap-2 overflow-auto pr-1" },
      props.workflows.items.map((item) => {
        const selected = item.id === props.selectedId;
        return h(
          "button",
          {
            key: item.id,
            type: "button",
            onClick: () => props.selectWorkflow(item.id),
            class: clsx(
              "group grid w-full gap-1 rounded-[1.25rem] px-4 py-3 text-left ring-1 active:scale-[0.98]",
              selected
                ? "bg-studio-accent text-white ring-studio-accent"
                : "bg-white/40 text-neutral-700 ring-black/5 hover:bg-white/80 dark:bg-white/[0.03] dark:text-neutral-200 dark:ring-white/10 dark:hover:bg-white/[0.06]",
              transition
            )
          },
          [
            h("span", { class: "flex min-w-0 items-center gap-2" }, [
              h("span", { class: "h-2.5 w-2.5 shrink-0 rounded-full ring-1 ring-black/10 dark:ring-white/20", style: { backgroundColor: workflowSwatch(item.color) } }),
              h("span", { class: "truncate font-mono text-sm" }, item.id)
            ]),
            h("span", { class: clsx("truncate text-xs", selected ? "text-white/70" : "text-neutral-500 dark:text-neutral-500") }, item.title || item.mode)
          ]
        );
      })
    )
  ]);
}

function Bezel(props: { children?: ComponentChildren; className?: string; innerClassName?: string }) {
  return h(
    "div",
    {
      class: clsx(
        "rounded-[2rem] bg-black/[0.04] p-1.5 shadow-[0_30px_60px_-20px_rgba(0,0,0,0.08)] ring-1 ring-black/5 dark:bg-white/[0.04] dark:ring-white/10",
        props.className
      )
    },
    h(
      "div",
      {
        class: clsx(
          "grid gap-6 rounded-[calc(2rem-0.375rem)] bg-white p-6 shadow-[inset_0_1px_1px_rgba(255,255,255,0.6)] dark:bg-neutral-950 dark:shadow-[inset_0_1px_1px_rgba(255,255,255,0.04)] sm:p-8",
          props.innerClassName
        )
      },
      props.children
    )
  );
}

function SurfacePill(props: { label: string; active: boolean; onClick: () => void }) {
  return h(
    "button",
    {
      type: "button",
      onClick: props.onClick,
      class: clsx(
        "rounded-full px-3 py-1.5 text-xs font-medium ring-1 active:scale-[0.98]",
        props.active
          ? "bg-neutral-950 text-white ring-neutral-950 dark:bg-white dark:text-neutral-950 dark:ring-white"
          : "bg-transparent text-neutral-500 ring-transparent hover:text-neutral-800 dark:text-neutral-400 dark:hover:text-neutral-100",
        transition
      )
    },
    props.label
  );
}

function ConfigResourceList(props: { selected: boolean }) {
  return h("div", { class: "grid gap-5" }, [
    h("div", {}, [
      h("p", { class: eyebrow }, "디스크"),
      h("p", { class: "mt-3 text-2xl font-medium text-neutral-950 dark:text-neutral-50" }, "Personal config")
    ]),
    h(
      "button",
      {
        type: "button",
        class: clsx(
          "group grid w-full gap-1 rounded-[1.25rem] px-4 py-3 text-left ring-1 active:scale-[0.98]",
          props.selected
            ? "bg-studio-accent text-white ring-studio-accent"
            : "bg-white/40 text-neutral-700 ring-black/5 dark:bg-white/[0.03] dark:text-neutral-200 dark:ring-white/10",
          transition
        )
      },
      [
        h("span", { class: "truncate text-sm font-medium" }, "local.toml"),
        h("span", { class: "truncate text-xs text-white/70" }, "<repo-root>/.wt/config/local.toml")
      ]
    )
  ]);
}

function ProfileResourceList(props: {
  profiles: ProfileInventory;
  selectedName: string;
  onSelect: (name: string) => void;
}) {
  if (props.profiles.items.length === 0) {
    return h("div", { class: "grid gap-5" }, [
      h("div", {}, [
        h("p", { class: eyebrow }, "디스크"),
        h("p", { class: "mt-3 text-2xl font-medium text-neutral-950 dark:text-neutral-50" }, "Profiles")
      ]),
      h("p", { class: "text-sm leading-6 text-neutral-500 dark:text-neutral-400" }, "profile.toml이 있는 profile이 없습니다.")
    ]);
  }

  return h("div", { class: "grid gap-5" }, [
    h("div", {}, [
      h("p", { class: eyebrow }, "디스크"),
      h("p", { class: "mt-3 text-2xl font-medium text-neutral-950 dark:text-neutral-50" }, `${props.profiles.items.length} Profiles`)
    ]),
    h(
      "div",
      { class: "grid max-h-[24rem] gap-2 overflow-auto pr-1" },
      props.profiles.items.map((item) => {
        const selected = item.name === props.selectedName;
        return h(
          "button",
          {
            key: item.path,
            type: "button",
            onClick: () => props.onSelect(item.name),
            class: clsx(
              "group grid w-full gap-1 rounded-[1.25rem] px-4 py-3 text-left ring-1 active:scale-[0.98]",
              selected
                ? "bg-studio-accent text-white ring-studio-accent"
                : "bg-white/40 text-neutral-700 ring-black/5 hover:bg-white/80 dark:bg-white/[0.03] dark:text-neutral-200 dark:ring-white/10 dark:hover:bg-white/[0.06]",
              transition
            )
          },
          [
            h("span", { class: "truncate text-sm font-medium" }, item.name),
            h("span", { class: clsx("truncate text-xs", selected ? "text-white/70" : "text-neutral-500 dark:text-neutral-500") }, "profile.toml")
          ]
        );
      })
    )
  ]);
}

function PromptResourceList(props: {
  profile: string;
  mode: PromptMode;
  onModeChange: (mode: PromptMode) => void;
}) {
  return h("div", { class: "grid gap-5" }, [
    h("div", {}, [
      h("p", { class: eyebrow }, "디스크"),
      h("p", { class: "mt-3 text-2xl font-medium text-neutral-950 dark:text-neutral-50" }, "Profile prompts")
    ]),
    h("div", { class: "grid gap-2" }, [
      h(
        "div",
        {
          class:
            "grid w-full gap-1 rounded-[1.25rem] bg-studio-accent px-4 py-3 text-left text-white ring-1 ring-studio-accent"
        },
        [
          h("span", { class: "truncate text-sm font-medium" }, props.profile.trim() || "profile"),
          h("span", { class: "truncate text-xs text-white/70" }, "<repo-root>/.wt/config/profiles/<name>/prompts")
        ]
      ),
      h(
        "div",
        { class: "grid gap-2 pl-3" },
        promptModes.map((mode) =>
          h(
            "button",
            {
              key: mode,
              type: "button",
              onClick: () => props.onModeChange(mode),
              class: clsx(
                "group grid w-full gap-1 rounded-[1rem] px-4 py-2 text-left ring-1 active:scale-[0.98]",
                props.mode === mode
                  ? "bg-neutral-950 text-white ring-neutral-950 dark:bg-white dark:text-neutral-950 dark:ring-white"
                  : "bg-white/40 text-neutral-700 ring-black/5 hover:bg-white/80 dark:bg-white/[0.03] dark:text-neutral-200 dark:ring-white/10 dark:hover:bg-white/[0.06]",
                transition
              )
            },
            [
              h("span", { class: "truncate text-sm font-medium" }, `${mode}.md`),
              h("span", { class: clsx("truncate text-xs", props.mode === mode ? "text-white/70 dark:text-neutral-500" : "text-neutral-500 dark:text-neutral-500") }, promptModeSummary(mode))
            ]
          )
        )
      )
    ])
  ]);
}

function IconButton(props: {
  label: string;
  iconComponent: IconComponent;
  onClick: () => void;
  disabled?: boolean;
}) {
  return h(
    "button",
    {
      type: "button",
      onClick: props.onClick,
      disabled: props.disabled,
      class: clsx(
        "group flex items-center gap-2 rounded-full bg-black/[0.04] py-2 pl-4 pr-2 text-sm font-medium text-neutral-700 ring-1 ring-black/5 active:scale-[0.98] disabled:opacity-45 dark:bg-white/[0.08] dark:text-neutral-200 dark:ring-white/10",
        transition
      )
    },
    [
      h("span", {}, props.label),
      h("span", { class: "grid h-8 w-8 place-items-center rounded-full bg-white/80 dark:bg-white/10" }, icon(props.iconComponent, "h-4 w-4 group-hover:translate-x-1 group-hover:-translate-y-px " + transition))
    ]
  );
}

function ActionButton(props: {
  label: string;
  iconComponent: IconComponent;
  onClick: () => void;
  disabled?: boolean;
  tone?: "primary" | "plain";
}) {
  const primary = props.tone === "primary";
  return h(
    "button",
    {
      type: "button",
      onClick: props.onClick,
      disabled: props.disabled,
      class: clsx(
        "group flex w-full items-center justify-between gap-4 rounded-full px-6 py-3 text-sm font-medium active:scale-[0.98] disabled:opacity-45 sm:w-auto",
        primary
          ? "bg-studio-accent text-white shadow-[0_18px_44px_-22px_rgba(59,130,246,0.9)]"
          : "bg-neutral-950 text-white dark:bg-white dark:text-neutral-950",
        transition
      )
    },
    [
      h("span", {}, props.label),
      h("span", { class: "grid h-8 w-8 place-items-center rounded-full bg-white/18 dark:bg-black/10" }, icon(props.iconComponent, "h-4 w-4 group-hover:translate-x-1 group-hover:-translate-y-px " + transition))
    ]
  );
}

function Field(props: {
  name: string;
  label: string;
  value: string;
  onInput: (value: string) => void;
  disabled?: boolean;
  className?: string;
}) {
  const id = `task-document-${props.name}`;
  return h("label", { class: clsx("grid gap-2 text-sm font-medium text-neutral-600 dark:text-neutral-300", props.className) }, [
    h("span", {}, props.label),
    h("input", {
      id,
      name: props.name,
      value: props.value,
      disabled: props.disabled,
      onInput: (event: Event) => props.onInput(formValue(event)),
      class: clsx(
        "h-12 rounded-full bg-white/70 px-5 text-neutral-950 shadow-[inset_0_1px_1px_rgba(255,255,255,0.75)] ring-1 ring-black/5 placeholder:text-neutral-400 focus:ring-2 focus:ring-studio-accent/50 disabled:text-neutral-500 dark:bg-neutral-900/70 dark:text-neutral-50 dark:ring-white/10",
        transition
      )
    })
  ]);
}

function MetaPill(props: { label: string; tone?: "blue" | "amber" | "neutral" }) {
  return h(
    "span",
    {
      class: clsx(
        "rounded-full px-3 py-1 text-[10px] font-medium uppercase tracking-[0.2em] ring-1",
        props.tone === "blue"
          ? "bg-studio-accent/10 text-studio-accent ring-studio-accent/20"
          : props.tone === "amber"
            ? "bg-amber-400/10 text-amber-700 ring-amber-500/20 dark:text-amber-300"
            : "bg-black/[0.04] text-neutral-500 ring-black/5 dark:bg-white/[0.04] dark:text-neutral-400 dark:ring-white/10"
      )
    },
    props.label
  );
}

function ValidationList(props: { items: string[] }) {
  return h("div", { class: "rounded-[1.25rem] bg-amber-400/10 p-4 text-sm text-amber-700 ring-1 ring-amber-500/20 dark:text-amber-300" }, [
    h("div", { class: "flex items-center gap-2 font-medium" }, [icon(StudioWarning, "h-4 w-4"), "검증"]),
    h("ul", { class: "mt-2 grid gap-1 border-l border-amber-500/30 pl-3" }, props.items.map((item) => h("li", { key: item }, item)))
  ]);
}

function MessagePanel(props: { message: string; tone: "error" }) {
  return h(
    Bezel,
    { className: "animate-[studio-spring_700ms_var(--ease-studio)_both]", innerClassName: "bg-amber-50 dark:bg-neutral-950" },
    [
      h("div", { class: "flex items-center gap-3 text-amber-700 dark:text-amber-300" }, [
        h("span", { class: "grid h-10 w-10 place-items-center rounded-full bg-amber-400/10 ring-1 ring-amber-500/20" }, icon(StudioWarning, "h-5 w-5")),
        h("strong", { class: "font-medium" }, "외부 변경 또는 검증 문제")
      ]),
      h("pre", { class: "overflow-auto whitespace-pre-wrap font-mono text-sm leading-6 text-neutral-700 dark:text-neutral-300" }, props.message)
    ]
  );
}

function PlanPreview(props: { plan: PreviewPlan }) {
  const lines = (props.plan.diff || "변경 사항 없음").split("\n");
  return h(Bezel, { className: "animate-[studio-spring_700ms_var(--ease-studio)_both]" }, [
    h("div", { class: "flex flex-col gap-4 md:flex-row md:items-center md:justify-between" }, [
      h("div", {}, [
        h("p", { class: eyebrow }, "Plan 미리보기"),
        h("div", { class: "mt-4 flex items-center gap-3" }, [
          h("span", { class: "grid h-10 w-10 place-items-center rounded-full bg-studio-accent/10 text-studio-accent ring-1 ring-studio-accent/20" }, icon(props.plan.valid ? StudioCheck : StudioWarning, "h-5 w-5")),
          h("strong", { class: "text-lg font-medium text-neutral-950 dark:text-neutral-50" }, props.plan.valid ? "유효한 Plan" : "유효하지 않은 Plan")
        ])
      ]),
      h("p", { class: "max-w-[34ch] [overflow-wrap:anywhere] text-sm leading-6 text-neutral-500 dark:text-neutral-400" }, props.plan.path)
    ]),
    props.plan.validation_errors.length > 0 && h(ValidationList, { items: props.plan.validation_errors }),
    h("div", { class: "overflow-hidden rounded-[1.5rem] bg-neutral-950 text-neutral-100 ring-1 ring-black/10 dark:bg-black dark:ring-white/10" }, [
      h("div", { class: "flex items-center justify-between px-5 py-3 text-[10px] uppercase tracking-[0.2em] text-neutral-400" }, [
        h("span", {}, "Unified diff"),
        h("span", {}, operationLabel(props.plan.operation))
      ]),
      h("pre", { class: "max-h-[30rem] overflow-auto px-0 pb-5 font-mono text-sm leading-6" }, lines.map((line, index) => diffLine(line, index)))
    ])
  ]);
}

function diffLine(line: string, index: number) {
  const tone = line.startsWith("+")
    ? "bg-blue-400/10 text-blue-100"
    : line.startsWith("-")
      ? "bg-amber-400/10 text-amber-100"
      : "text-neutral-300";
  return h(
    "span",
    {
      key: `${index}-${line}`,
      class: clsx("block min-h-6 whitespace-pre px-5 opacity-100 transition-[transform,opacity] duration-700 ease-[cubic-bezier(0.32,0.72,0,1)]", tone),
      style: { transitionDelay: `${Math.min(index * 40, 640)}ms` }
    },
    line || " "
  );
}

function metric(label: string, value: string) {
  return h("div", { key: label }, [
    h("dt", { class: "text-[10px] font-medium uppercase tracking-[0.2em] text-neutral-400 dark:text-neutral-500" }, label),
    h("dd", { class: "mt-1 [overflow-wrap:anywhere] text-neutral-700 dark:text-neutral-300" }, value)
  ]);
}

function buildDetailMetrics(
  currentPath: string,
  draft: EditorDraft,
  selected: TaskDocumentItem | null,
  plan: PlanResponse | null,
  draftIssues: string[],
  planStatus: PlanStatus
): DetailMetric[] {
  const origin = selected?.document.origin || documentFromDraft(draft).origin;
  const metrics: DetailMetric[] = [
    { label: "대상", value: currentPath },
    { label: "Branch", value: draft.branch.trim() || draft.slug.trim() || "task" },
    { label: "Body", value: bodySummary(draft.body) },
    { label: "검증", value: validationSummary(plan, draftIssues, planStatus) }
  ];

  if (origin?.provider || origin?.id) {
    metrics.push({ label: "Origin provider", value: origin.provider || "없음" });
    metrics.push({ label: "Origin id", value: origin.id || "없음" });
    return metrics;
  }

  metrics.push({ label: "Modified", value: formatKoreanMtime(selected?.fingerprint.mtime_ns) });
  metrics.push({ label: "Hash", value: selected?.fingerprint.hash.slice(0, 12) || "새 파일" });
  return metrics;
}

function buildConfigDetailMetrics(
  candidate: string,
  draft: ConfigDraft,
  plan: PersonalConfigPlanResponse | null,
  planStatus: PlanStatus
): DetailMetric[] {
  return [
    { label: "대상", value: "<repo-root>/.wt/config/local.toml" },
    { label: "Sections", value: configSummary(draft) },
    { label: "Candidate", value: bodySummary(candidate) },
    { label: "검증", value: configValidationSummary(plan, planStatus) },
    { label: "Hash", value: plan?.fingerprint.hash.slice(0, 12) || "baseline" }
  ];
}

function buildProfileDetailMetrics(
  profile: string,
  candidate: string,
  draft: ProfileDraft,
  plan: PersonalConfigPlanResponse | null,
  planStatus: PlanStatus
): DetailMetric[] {
  return [
    { label: "대상", value: profileDisplayPath(profile) },
    { label: "Profile", value: profile.trim() || "없음" },
    { label: "Sections", value: profileSummary(draft) },
    { label: "Candidate", value: bodySummary(candidate) },
    { label: "검증", value: configValidationSummary(plan, planStatus) },
    { label: "Hash", value: plan?.fingerprint.hash.slice(0, 12) || "baseline" }
  ];
}

function buildWorkflowDetailMetrics(workflow: WorkflowDetail | null, total: number): DetailMetric[] {
  if (!workflow) {
    return [
      { label: "대상", value: "<repo-root>/.wt/execution/workflows/<id>.toml" },
      { label: "Workflows", value: `${total.toLocaleString("ko-KR")}개` },
      { label: "상태", value: "선택 대기" }
    ];
  }
  return [
    { label: "대상", value: workflow.path },
    { label: "Mode", value: workflow.mode },
    { label: "Policy", value: `${workflow.policy.pull_request} / ${workflow.policy.landing}` },
    { label: "Tasks", value: `${workflow.tasks.length.toLocaleString("ko-KR")}개` },
    { label: "Updated", value: workflow.updated_at }
  ];
}

function buildPromptDetailMetrics(
  profile: string,
  mode: PromptMode,
  candidate: string,
  plan: ProfilePromptPlanResponse | null,
  planStatus: PlanStatus
): DetailMetric[] {
  return [
    { label: "대상", value: promptDisplayPath(profile, mode) },
    { label: "Profile", value: profile.trim() || "없음" },
    { label: "Mode", value: mode },
    { label: "Markdown", value: bodySummary(candidate) },
    { label: "검증", value: promptValidationSummary(plan, planStatus) },
    { label: "Hash", value: plan?.fingerprint.hash.slice(0, 12) || "baseline" }
  ];
}

function bodySummary(body: string) {
  const chars = body.length;
  const lines = body.length === 0 ? 0 : body.split(/\r\n|\r|\n/).length;
  return `${chars.toLocaleString("ko-KR")}자 / ${lines.toLocaleString("ko-KR")}줄`;
}

function configValidationSummary(plan: PersonalConfigPlanResponse | null, planStatus: PlanStatus) {
  if (planStatus === "planning") {
    return "Plan 갱신 중";
  }
  if (planStatus === "stale" || plan?.baseline_stale) {
    return "Stale";
  }
  if (!plan) {
    return "Plan 대기";
  }
  return plan.validation_errors.length === 0 ? "검증 통과" : `검증 오류 ${plan.validation_errors.length}개`;
}

function promptValidationSummary(plan: ProfilePromptPlanResponse | null, planStatus: PlanStatus) {
  if (planStatus === "planning") {
    return "Plan 갱신 중";
  }
  if (planStatus === "stale" || plan?.baseline_stale) {
    return "Stale";
  }
  return plan ? "검증 없음" : "Plan 대기";
}

function validationSummary(plan: PlanResponse | null, draftIssues: string[], planStatus: PlanStatus) {
  if (draftIssues.length > 0) {
    return `입력 오류 ${draftIssues.length}개`;
  }
  if (planStatus === "planning") {
    return "Plan 갱신 중";
  }
  if (planStatus === "stale") {
    return "Stale";
  }
  if (!plan) {
    return "Plan 대기";
  }
  return plan.valid ? "검증 통과" : `검증 오류 ${plan.validation_errors.length}개`;
}

function configPlanSummaryLabel(planStatus: PlanStatus, plan: PersonalConfigPlanResponse | null) {
  if (planStatus === "planning") {
    return "Plan 갱신 중";
  }
  if (planStatus === "stale" || plan?.baseline_stale) {
    return "Stale";
  }
  return plan && plan.validation_errors.length === 0 ? "유효한 Diff" : "Plan 대기";
}

function promptPlanSummaryLabel(planStatus: PlanStatus, plan: ProfilePromptPlanResponse | null) {
  if (planStatus === "planning") {
    return "Plan 갱신 중";
  }
  if (planStatus === "stale" || plan?.baseline_stale) {
    return "Stale";
  }
  return plan ? "유효한 Diff" : "Plan 대기";
}

function planSummaryLabel(planStatus: PlanStatus, plan: PlanResponse | null) {
  if (planStatus === "planning") {
    return "Plan 갱신 중";
  }
  if (planStatus === "stale") {
    return "Stale";
  }
  return plan?.valid ? "유효한 Diff" : "Plan 대기";
}

function workflowSummaryLabel(loading: boolean, workflow: WorkflowDetail | null) {
  if (loading) {
    return "읽는 중";
  }
  return workflow ? "Loaded" : "선택 대기";
}

function formatKoreanMtime(mtimeNs?: string | null) {
  if (!mtimeNs) {
    return "새 파일";
  }
  try {
    const ms = Number(BigInt(mtimeNs) / 1_000_000n);
    return new Intl.DateTimeFormat("ko-KR", {
      month: "short",
      day: "numeric",
      hour: "2-digit",
      minute: "2-digit"
    }).format(new Date(ms));
  } catch {
    return "수정 시각 없음";
  }
}

function operationLabel(operation: PreviewPlan["operation"]) {
  if (operation === "config") {
    return "config";
  }
  if (operation === "prompt") {
    return "prompt";
  }
  return operation === "create" ? "생성" : "수정";
}

function taskPreview(plan: PlanResponse): PreviewPlan {
  return {
    path: plan.path,
    operation: plan.operation,
    valid: plan.valid,
    validation_errors: plan.validation_errors,
    diff: plan.diff
  };
}

function personalConfigPreview(plan: PersonalConfigPlanResponse): PreviewPlan {
  return {
    path: "<repo-root>/.wt/config/local.toml",
    operation: "config",
    valid: plan.validation_errors.length === 0 && !plan.baseline_stale,
    validation_errors: plan.validation_errors,
    diff: plan.diff
  };
}

function profileConfigPreview(profile: string, plan: PersonalConfigPlanResponse): PreviewPlan {
  return {
    path: profileDisplayPath(profile),
    operation: "config",
    valid: plan.validation_errors.length === 0 && !plan.baseline_stale,
    validation_errors: plan.validation_errors,
    diff: plan.diff
  };
}

function profilePromptPreview(profile: string, mode: PromptMode, plan: ProfilePromptPlanResponse): PreviewPlan {
  return {
    path: promptDisplayPath(profile, mode),
    operation: "prompt",
    valid: plan.validation_errors.length === 0 && !plan.baseline_stale,
    validation_errors: plan.validation_errors,
    diff: plan.diff
  };
}

function icon(IconComponent: IconComponent, className: string) {
  return h(IconComponent, { size: 20, weight: "light", className });
}

function draftFromItem(item: TaskDocumentItem): EditorDraft {
  return {
    slug: item.key,
    title: item.document.title || "",
    branch: item.document.branch || "",
    body: item.document.body || "",
    originProvider: item.document.origin?.provider || "",
    originId: item.document.origin?.id || ""
  };
}

function draftsEqual(left: EditorDraft, right: EditorDraft) {
  return (
    left.slug === right.slug &&
    left.title === right.title &&
    left.branch === right.branch &&
    left.body === right.body &&
    left.originProvider === right.originProvider &&
    left.originId === right.originId
  );
}

function documentFromDraft(draft: EditorDraft): TaskDocument {
  const slug = draft.slug.trim() || "task";
  const origin =
    draft.originProvider.trim() || draft.originId.trim()
      ? { provider: draft.originProvider.trim(), id: draft.originId.trim() }
      : null;
  return {
    title: draft.title,
    branch: draft.branch.trim() || slug,
    body: draft.body,
    origin
  };
}

function validateDraft(mode: Mode, draft: EditorDraft) {
  const issues = [];
  if (mode === "create" && !/^[a-z0-9-]+$/.test(draft.slug.trim())) {
    issues.push("Slug는 소문자, 숫자, 하이픈만 사용할 수 있습니다.");
  }
  const titleLength = draft.title.trim().length;
  if (titleLength === 0 || titleLength > 120) {
    issues.push("Title은 1~120자여야 합니다.");
  }
  return issues;
}

function targetPath(mode: Mode, draft: EditorDraft, selected: TaskDocumentItem | null) {
  if (mode === "update" && selected) {
    return selected.path;
  }
  const slug = draft.slug.trim() || "task";
  return slug.endsWith(".toml") ? slug : `${slug}.toml`;
}

function planRequestSignature(mode: Mode, path: string, draft: EditorDraft) {
  return JSON.stringify({
    path,
    mode,
    draft
  });
}

function statusDescriptor(planStatus: PlanStatus, plan: PlanResponse | null) {
  if (planStatus === "stale") {
    return { label: "Stale (재plan 필요)", tone: "amber" as const };
  }
  if (planStatus === "planning") {
    return { label: "Plan 갱신 중", tone: "blue" as const, pulse: true };
  }
  if (plan?.valid) {
    return { label: "적용 가능", tone: "blue" as const };
  }
  return { label: "미저장", tone: "neutral" as const };
}

function configStatusDescriptor(planStatus: PlanStatus, plan: PersonalConfigPlanResponse | null) {
  if (planStatus === "stale" || plan?.baseline_stale) {
    return { label: "Stale (재plan 필요)", tone: "amber" as const };
  }
  if (planStatus === "planning") {
    return { label: "Plan 갱신 중", tone: "blue" as const, pulse: true };
  }
  if (plan && plan.validation_errors.length === 0) {
    return { label: "적용 가능", tone: "blue" as const };
  }
  return { label: "미저장", tone: "neutral" as const };
}

function promptStatusDescriptor(planStatus: PlanStatus, plan: ProfilePromptPlanResponse | null) {
  if (planStatus === "stale" || plan?.baseline_stale) {
    return { label: "Stale (재plan 필요)", tone: "amber" as const };
  }
  if (planStatus === "planning") {
    return { label: "Plan 갱신 중", tone: "blue" as const, pulse: true };
  }
  if (plan && plan.validation_errors.length === 0) {
    return { label: "적용 가능", tone: "blue" as const };
  }
  return { label: "미저장", tone: "neutral" as const };
}

function workflowStatusDescriptor(loading: boolean, workflow: WorkflowDetail | null) {
  if (loading) {
    return { label: "Workflow 읽는 중", tone: "blue" as const, pulse: true };
  }
  if (workflow) {
    return { label: "읽기 전용", tone: "blue" as const };
  }
  return { label: "선택 대기", tone: "neutral" as const };
}

function statusClass(tone: "blue" | "amber" | "neutral", pulse?: boolean) {
  return clsx(
    "inline-flex w-fit rounded-full px-3 py-1 text-[10px] font-medium uppercase tracking-[0.2em] ring-1",
    pulse && "studio-status-pulse",
    tone === "amber"
      ? "bg-amber-400/10 text-amber-700 ring-amber-500/20 dark:text-amber-300"
      : tone === "blue"
        ? "bg-studio-accent/10 text-studio-accent ring-studio-accent/20"
        : "bg-black/[0.04] text-neutral-500 ring-black/5 dark:bg-white/[0.04] dark:text-neutral-400 dark:ring-white/10"
  );
}

function surfaceFromHash(): SurfaceMode {
  if (window.location.hash === "#config") {
    return "config";
  }
  if (window.location.hash.match(/^#profile\/[^/]+\/prompts\/[^/]+$/)) {
    return "prompts";
  }
  if (window.location.hash === "#profile" || window.location.hash.match(/^#profile\/[^/]+$/)) {
    return "profiles";
  }
  if (window.location.hash.startsWith("#workflow")) {
    return "workflow";
  }
  return "tasks";
}

function taskPathFromHash(items: TaskDocumentItem[]) {
  const match = window.location.hash.match(/^#task\/(.+)$/);
  if (!match) {
    return "";
  }
  const key = decodeURIComponent(match[1]);
  return items.find((item) => item.key === key || item.path.endsWith(`/${key}.toml`) || item.path.endsWith(`${key}.toml`))?.path || "";
}

function workflowIdFromHash() {
  const match = window.location.hash.match(/^#workflow\/(.+)$/);
  return match ? decodeURIComponent(match[1]) : "";
}

function promptLocationFromHash(): { profile: string; mode: PromptMode } {
  const match = window.location.hash.match(/^#profile\/([^/]+)\/prompts\/([^/]+)$/);
  const profile = match ? decodeURIComponent(match[1]) : "codex";
  const candidateMode = match ? decodeURIComponent(match[2]) : "workflow";
  const mode = isPromptMode(candidateMode) ? candidateMode : "workflow";
  return { profile, mode };
}

function profileNameFromHash() {
  const match = window.location.hash.match(/^#profile\/([^/]+)$/);
  return match ? decodeURIComponent(match[1]) : "";
}

function promptHash(profile: string, mode: PromptMode) {
  const safeProfile = profile.trim() || "codex";
  return `profile/${encodeURIComponent(safeProfile)}/prompts/${mode}`;
}

function isPromptMode(mode: string): mode is PromptMode {
  return promptModes.includes(mode as PromptMode);
}

function promptDisplayPath(profile: string, mode: PromptMode) {
  return `<repo-root>/.wt/config/profiles/${profile.trim() || "<name>"}/prompts/${mode}.md`;
}

function profileDisplayPath(profile: string) {
  return `<repo-root>/.wt/config/profiles/${profile.trim() || "<name>"}/profile.toml`;
}

function promptModeSummary(mode: PromptMode) {
  switch (mode) {
    case "workflow":
      return "workflow task scope";
    case "issue":
      return "issue worktree scope";
    case "branch":
      return "branch worktree scope";
    case "pr":
      return "pull request scope";
    case "common":
      return "shared scope";
  }
}

function workflowSwatch(color?: string | null) {
  const palette: Record<string, string> = {
    red: "#ef4444",
    crimson: "#dc143c",
    orange: "#f97316",
    amber: "#f59e0b",
    olive: "#84cc16",
    green: "#22c55e",
    teal: "#14b8a6",
    aqua: "#06b6d4",
    blue: "#3b82f6",
    navy: "#1d4ed8",
    indigo: "#6366f1",
    purple: "#a855f7",
    magenta: "#d946ef",
    rose: "#f43f5e",
    brown: "#92400e",
    charcoal: "#374151"
  };
  return color ? palette[color.toLowerCase()] || "#3b82f6" : "#3b82f6";
}

type ApiFailure = Error & {
  status?: number;
  diff?: string;
};

async function api<T = unknown>(path: string, options: RequestInit): Promise<T> {
  const response = await fetch(path, {
    credentials: "same-origin",
    headers: { "Content-Type": "application/json" },
    ...options
  });
  const body = await response.json().catch(() => ({}));
  if (!response.ok) {
    const error = new Error(body.error || `${response.status} ${response.statusText}`) as ApiFailure;
    error.status = response.status;
    error.diff = body.diff;
    throw error;
  }
  return body as T;
}

function errorMessage(err: unknown) {
  return err instanceof Error ? err.message : String(err);
}

function isAbortError(err: unknown) {
  return err instanceof DOMException && err.name === "AbortError";
}

function formValue(event: Event) {
  return (event.currentTarget as HTMLInputElement | HTMLTextAreaElement).value;
}

render(h(App, {}), document.getElementById("app") as HTMLElement);
