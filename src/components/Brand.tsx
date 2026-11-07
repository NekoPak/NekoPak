/**
 * NekoPak marks — original artwork (a parcel whose flaps are cat ears),
 * drawn in the current accent. Unrelated to PikPak's or Piko's logos.
 */

export function LogoMark({ size = 28 }: { size?: number }) {
  return (
    <svg class="logo-mark" width={size} height={size} viewBox="0 0 32 32" aria-hidden="true" focusable="false">
      <rect width="32" height="32" rx="9" fill="var(--primary)" />
      <path d="M7.5 13.2 10.2 7.6l3.5 4.1h4.6l3.5-4.1 2.7 5.6v9.3a2.2 2.2 0 0 1-2.2 2.2H9.7a2.2 2.2 0 0 1-2.2-2.2Z" fill="var(--on-primary)" />
      <path d="M7.5 15h17" stroke="var(--primary)" stroke-width="1.3" opacity=".35" />
      <path d="M11.6 19.3q1.2-1.2 2.4 0M18 19.3q1.2-1.2 2.4 0" fill="none" stroke="var(--primary)" stroke-width="1.6" stroke-linecap="round" />
    </svg>
  );
}

/** A cat napping in an open box — empty states only. */
export function BoxCat({ size = 128, awake = false }: { size?: number; awake?: boolean }) {
  return (
    <svg class="boxcat" width={size} height={(size * 84) / 140} viewBox="0 0 140 84" aria-hidden="true" focusable="false" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round">
      <path d="M24 47h92v29a3 3 0 0 1-3 3H27a3 3 0 0 1-3-3Z" fill="var(--surface-high)" />
      <path d="M44 46c0-14 11-22 26-22s26 8 26 22" fill="var(--surface-lowest)" />
      <path d="M48 33 46 17l13 9M92 33l2-16-13 9" fill="var(--surface-lowest)" />
      {awake ? (
        <>
          <circle cx="62" cy="39" r="1.8" fill="currentColor" stroke="none" />
          <circle cx="78" cy="39" r="1.8" fill="currentColor" stroke="none" />
        </>
      ) : (
        <path d="M58 40q3.5-3 7 0M75 40q3.5-3 7 0" />
      )}
      <path d="M68 44.5h4l-2 2Z" fill="currentColor" stroke-width="1" />
      <path d="M52 47q5-6 10 0M78 47q5-6 10 0" fill="var(--surface-lowest)" />
      <path d="M24 47 12 37M116 47l12-10" />
    </svg>
  );
}
