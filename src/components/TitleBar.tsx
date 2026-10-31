import { getCurrentWindow } from '@tauri-apps/api/window'
import { Mic, Minus, Power, Settings, Speaker, Square, X } from 'lucide-react'
import { keyLabel } from '../lib/hotkeys'
import { currentDevice, dismiss, setState, updateConfig, useStore } from '../lib/store'
import { useT } from '../i18n'
import { Select } from './controls'
import { Logo } from './ui'

const win = getCurrentWindow()

export function TitleBar(): React.JSX.Element {
  const t = useT()
  const devices = useStore((s) => s.engine?.devices)
  const device = useStore((s) => currentDevice(s))
  const mic = !!device?.input
  // El botón enciende y apaga lo que se edita: el EQ de las salidas o el procesado del micrófono
  const off = useStore((s) => (mic ? !!s.config?.mic.bypass : !!s.config?.bypass))
  const hotkey = useStore((s) => s.config?.hotkeys.bypass)
  const list = (devices ?? []).filter((d) => !d.unplugged)
  const toggle = (): void => {
    const cfg = useStore.getState().config
    if (!cfg) return
    if (mic) updateConfig({ mic: { ...cfg.mic, bypass: !cfg.mic.bypass } })
    else updateConfig({ bypass: !cfg.bypass })
  }
  const tip = mic ? t(off ? 'titlebar.micBypassOn' : 'titlebar.micBypassOff') : `${t(off ? 'titlebar.bypassOn' : 'titlebar.bypassOff')}${hotkey ? ` · ${keyLabel(hotkey)}` : ''}`

  return (
    <header className="titlebar" data-tauri-drag-region>
      <div className="brand" data-tauri-drag-region>
        <Logo />
        <span data-tauri-drag-region>
          Poxi<b>EQ</b>
        </span>
      </div>
      {device && (
        <div className={`no-drag device-pick ${mic ? 'mic' : ''}`}>
          <span className={`dot ${device.enabled ? 'ok' : ''}`} />
          <span className="kind" data-tip={t(mic ? 'titlebar.micTip' : 'titlebar.outputTip')}>
            {mic ? <Mic size={15} /> : <Speaker size={15} />}
          </span>
          <Select
            size="sm"
            value={device.id}
            label={t('titlebar.device')}
            options={list.map((d) => ({
              value: d.id,
              label: `${d.name} · ${d.device}${d.default ? ` (${t('device.default')})` : ''}`,
              group: t(d.input ? 'titlebar.inputs' : 'titlebar.outputs')
            }))}
            onChange={(id) => {
              setState({ selected: null })
              updateConfig({ device: id })
            }}
          />
        </div>
      )}
      <div className="spacer" data-tauri-drag-region />
      <button className={`power no-drag ${off ? 'bypassed' : ''}`} onClick={toggle} data-tip={tip} aria-label={tip} aria-pressed={!off}>
        <Power size={15} />
        <span>{mic ? 'MIC' : 'EQ'}</span>
      </button>
      <button className="icon-btn no-drag" onClick={() => setState({ settingsOpen: true })} data-tip={t('titlebar.settings')} aria-label={t('titlebar.settings')}>
        <Settings size={16} />
      </button>
      <div className="win-controls">
        <button onClick={() => void win.minimize()} aria-label={t('titlebar.minimize')}>
          <Minus size={16} />
        </button>
        <button onClick={() => void win.toggleMaximize()} aria-label={t('titlebar.maximize')}>
          <Square size={12} />
        </button>
        <button className="close" onClick={() => void win.close()} aria-label={t('titlebar.close')}>
          <X size={17} />
        </button>
      </div>
    </header>
  )
}

export function Toasts(): React.JSX.Element {
  const toasts = useStore((s) => s.toasts)
  return (
    <div className="toasts" aria-live="polite">
      {toasts.map((x) => (
        <div key={x.id} className={`toast ${x.kind}`}>
          <ToastIcon kind={x.kind} />
          {x.text}
          {x.action && (
            <button
              className="btn btn-sm toast-action"
              onClick={() => {
                dismiss(x.id)
                x.action?.run()
              }}
            >
              {x.action.label}
            </button>
          )}
          {x.secs ? <span className="bar" style={{ animationDuration: `${x.secs}s` }} /> : null}
        </div>
      ))}
    </div>
  )
}

function ToastIcon({ kind }: { kind: 'ok' | 'err' | 'info' }): React.JSX.Element {
  return (
    <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.4" strokeLinecap="round" strokeLinejoin="round">
      {kind === 'ok' && <path d="M20 6 9 17l-5-5" />}
      {kind === 'err' && (
        <>
          <circle cx="12" cy="12" r="9" />
          <path d="M12 8v4M12 16h.01" />
        </>
      )}
      {kind === 'info' && (
        <>
          <circle cx="12" cy="12" r="9" />
          <path d="M12 16v-4M12 8h.01" />
        </>
      )}
    </svg>
  )
}
