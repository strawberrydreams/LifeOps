<script lang="ts">
  import { onMount, onDestroy } from "svelte";
  import { Editor } from "@tiptap/core";
  import StarterKit from "@tiptap/starter-kit";
  import Link from "@tiptap/extension-link";

  let {
    value,
    onchange,
    id,
    labelledby,
    describedby,
  }: {
    value: string | null;
    onchange: (html: string) => void;
    id?: string;
    labelledby?: string;
    describedby?: string;
  } = $props();

  let element = $state<HTMLDivElement>();
  let editor: Editor | undefined = $state();

  onMount(() => {
    editor = new Editor({
      element: element!,
      extensions: [StarterKit, Link.configure({ openOnClick: false })],
      content: value ?? "",
      onUpdate: ({ editor }) => onchange(editor.getHTML()),
    });
  });

  $effect(() => {
    if (!editor) return;

    const next = value ?? "";
    const current = editor.getHTML();
    const normalizedNext = next === "" ? "<p></p>" : next;

    if (current !== normalizedNext) {
      editor.commands.setContent(next, false);
    }
  });

  onDestroy(() => editor?.destroy());

  function cmd(fn: () => void) {
    fn();
  }
</script>

<div class="note-editor" {id} role="group" aria-labelledby={labelledby} aria-describedby={describedby}>
  {#if editor}
    <div class="toolbar">
      <button type="button" onclick={() => cmd(() => editor!.chain().focus().toggleBold().run())}
        ><b>B</b></button
      >
      <button type="button" onclick={() => cmd(() => editor!.chain().focus().toggleItalic().run())}
        ><i>I</i></button
      >
      <button
        type="button"
        onclick={() => cmd(() => editor!.chain().focus().toggleHeading({ level: 2 }).run())}>H2</button
      >
      <button type="button" onclick={() => cmd(() => editor!.chain().focus().toggleBulletList().run())}
        >• 목록</button
      >
      <button
        type="button"
        onclick={() => {
          const url = prompt("링크 URL");
          if (url) editor!.chain().focus().setLink({ href: url }).run();
        }}>링크</button
      >
    </div>
  {/if}
  <div bind:this={element}></div>
</div>
